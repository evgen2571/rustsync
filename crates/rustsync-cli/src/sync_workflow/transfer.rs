use super::*;

pub(super) struct DownloadedFiles {
    pub contents: BTreeMap<String, Vec<u8>>,
    pub blob_count: usize,
}

impl<R: SyncRemote> SyncWorkflow<R> {
    pub(super) async fn push_with_expected_head(
        &self,
        previous_head: WorkspaceHead,
        previous_manifest: &Manifest,
    ) -> SyncWorkflowResult<PushReport> {
        let workspace_id = self.engine.workspace_id().clone();
        let manifest = self.engine.load_staged_manifest()?;
        let changed_files = changed_files(&manifest);

        let mut uploaded_blobs = 0usize;
        let mut reused_blobs = 0usize;
        let mut remote_manifest = manifest.clone();
        let mut remote_blob_ids: BTreeMap<_, _> = unique_file_remote_blob_ids(previous_manifest)?
            .into_iter()
            .map(|file| (file.content_hash, file.blob_ids))
            .collect();
        let crypto = self.engine.workspace().crypto();
        let overhead = crypto.encrypt_bytes(&[])?.to_binary_bytes()?.len();
        let chunk_size = rustsync_protocol::MAX_ENCRYPTED_OBJECT_BYTES
            .checked_sub(overhead)
            .filter(|size| *size > 0)
            .ok_or_else(|| {
                std::io::Error::other("encryption framing leaves no room for file content")
            })?;
        let mut seen_hashes = BTreeSet::new();
        for blob in self.engine.staged_blobs_for_manifest(&manifest)? {
            if !seen_hashes.insert(blob.content_hash.clone()) {
                continue;
            }
            if let Some(ids) = remote_blob_ids.get(&blob.content_hash) {
                reused_blobs += ids.len();
                continue;
            }

            let chunks: Vec<&[u8]> = if blob.bytes.is_empty() {
                vec![&[]]
            } else {
                blob.bytes.chunks(chunk_size).collect()
            };
            let mut ids = Vec::with_capacity(chunks.len());
            for chunk in chunks {
                let encrypted_blob_bytes = crypto.encrypt_bytes(chunk)?.to_binary_bytes()?;
                let blob_id = BlobId::from_content(&encrypted_blob_bytes);
                let transferred_bytes = encrypted_blob_bytes.len();
                let response = self
                    .remote
                    .upload_blob(&workspace_id, &blob_id, encrypted_blob_bytes)
                    .await
                    .map_err(boxed_error)?;
                self.transferred(TransferDirection::Upload, transferred_bytes);
                count_upload_response(response, &mut uploaded_blobs, &mut reused_blobs);
                ids.push(blob_id);
            }
            remote_blob_ids.insert(blob.content_hash, ids);
        }

        for entry in remote_manifest.entries.values_mut() {
            let ManifestEntry::File(file) = entry else {
                continue;
            };
            let ids = &remote_blob_ids[&file.content_hash];
            if ids.len() == 1 {
                file.remote_blob_id = Some(ids[0].clone());
                file.remote_chunk_ids.clear();
            } else {
                file.remote_blob_id = None;
                file.remote_chunk_ids = ids.clone();
            }
        }

        let manifest_bytes = manifest_to_json_bytes(&remote_manifest)?;
        let encrypted_manifest = self
            .engine
            .workspace()
            .crypto()
            .encrypt_bytes(&manifest_bytes)?;
        let encrypted_manifest_bytes = encrypted_manifest.to_binary_bytes()?;
        let manifest_id = ManifestId::from_content(&encrypted_manifest_bytes);
        let manifest_response = self
            .remote
            .upload_manifest(&workspace_id, &manifest_id, encrypted_manifest_bytes)
            .await
            .map_err(boxed_error)?;
        let mut uploaded_manifests = 0usize;
        let mut reused_manifests = 0usize;
        count_upload_response(
            manifest_response,
            &mut uploaded_manifests,
            &mut reused_manifests,
        );

        let updated_head = self
            .remote
            .update_workspace_head(&workspace_id, previous_head.revision, &manifest_id)
            .await
            .map_err(boxed_error)?;

        Ok(PushReport {
            workspace_id,
            manifest_id,
            uploaded_blobs,
            reused_blobs,
            uploaded_manifests,
            reused_manifests,
            changed_files,
            previous_head_revision: previous_head.revision,
            updated_head_revision: updated_head.revision,
        })
    }

    pub async fn remote_status(&self) -> SyncWorkflowResult<WorkspaceHead> {
        self.remote
            .fetch_workspace_head(self.engine.workspace_id())
            .await
            .map_err(boxed_error)
    }

    pub(super) async fn download_manifest_blobs(
        &self,
        workspace_id: &WorkspaceId,
        manifest: &Manifest,
        available_hashes: &BTreeSet<&str>,
    ) -> SyncWorkflowResult<DownloadedFiles> {
        let mut blobs = BTreeMap::new();
        let mut blob_count = 0;
        for RemoteFile {
            content_hash,
            size: expected_size,
            blob_ids,
        } in unique_file_remote_blob_ids(manifest)?
        {
            if available_hashes.contains(content_hash.as_str()) {
                continue;
            }
            let mut contents = Vec::new();
            for blob_id in blob_ids {
                let bytes = self
                    .remote
                    .download_blob(workspace_id, &blob_id)
                    .await
                    .map_err(boxed_error)?;
                verify_blob_id(&bytes, &blob_id)?;
                self.transferred(TransferDirection::Download, bytes.len());
                blob_count += 1;
                let chunk = self.decrypt_remote_object_bytes(&bytes)?;
                if chunk.len() as u64 > expected_size.saturating_sub(contents.len() as u64) {
                    return Err(std::io::Error::other(
                        "downloaded chunks exceed manifest file size",
                    )
                    .into());
                }
                contents.extend_from_slice(&chunk);
            }
            use sha2::Digest;
            if contents.len() as u64 != expected_size
                || hex::encode(sha2::Sha256::digest(&contents)) != content_hash
            {
                return Err(
                    std::io::Error::other("downloaded file size or content hash mismatch").into(),
                );
            }
            blobs.insert(content_hash, contents);
        }
        Ok(DownloadedFiles {
            contents: blobs,
            blob_count,
        })
    }
}

struct RemoteFile {
    content_hash: String,
    size: u64,
    blob_ids: Vec<BlobId>,
}

fn unique_file_remote_blob_ids(manifest: &Manifest) -> SyncWorkflowResult<Vec<RemoteFile>> {
    let mut blobs = BTreeMap::new();
    for entry in manifest.entries.values() {
        let ManifestEntry::File(file) = entry else {
            continue;
        };
        let ids = match (&file.remote_blob_id, file.remote_chunk_ids.is_empty()) {
            (Some(id), true) => vec![id.clone()],
            (None, false) => file.remote_chunk_ids.clone(),
            _ => return Err(std::io::Error::other(format!(
                "remote manifest file entry for content hash {} has missing remote_blob_id or conflicting chunk references",
                file.content_hash
            )).into()),
        };
        let value = (file.size, ids);
        if let Some(previous) = blobs.insert(file.content_hash.clone(), value.clone())
            && previous != value
        {
            return Err(std::io::Error::other(
                "inconsistent references for duplicate file content hash",
            )
            .into());
        }
    }

    Ok(blobs
        .into_iter()
        .map(|(content_hash, (size, blob_ids))| RemoteFile {
            content_hash,
            size,
            blob_ids,
        })
        .collect())
}
