use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use super::{DeviceError, DeviceIdentity, DeviceRecord, DeviceResult, DeviceStatus};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceRegistry {
    pub workspace_id: String,
    devices: BTreeMap<String, DeviceRecord>,
}

impl DeviceRegistry {
    pub fn new(workspace_id: impl Into<String>) -> Self {
        Self {
            workspace_id: workspace_id.into(),
            devices: BTreeMap::new(),
        }
    }

    pub fn with_owner(
        workspace_id: impl Into<String>,
        owner: &DeviceIdentity,
    ) -> DeviceResult<Self> {
        owner.validate()?;

        let mut registry = Self::new(workspace_id);
        registry.insert(owner.public_record(DeviceStatus::Active))?;

        Ok(registry)
    }

    pub fn insert(&mut self, record: DeviceRecord) -> DeviceResult<()> {
        record.validate()?;

        if self.devices.contains_key(&record.device_id) {
            return Err(DeviceError::AlreadyExists(record.device_id));
        }

        self.devices.insert(record.device_id.clone(), record);

        Ok(())
    }

    pub fn insert_pending(&mut self, mut record: DeviceRecord) -> DeviceResult<()> {
        record.status = DeviceStatus::Pending;
        self.insert(record)
    }

    pub fn activate(&mut self, device_id: &str) -> DeviceResult<()> {
        let device = self.get_mut(device_id)?;

        if device.is_revoked() {
            return Err(DeviceError::DeviceRevoked {
                device_id: device_id.to_string(),
            });
        }

        device.activate();

        Ok(())
    }

    pub fn revoke(&mut self, device_id: &str) -> DeviceResult<()> {
        let device = self.get_mut(device_id)?;
        device.revoke();

        Ok(())
    }

    pub fn remove(&mut self, device_id: &str) -> DeviceResult<DeviceRecord> {
        self.devices
            .remove(device_id)
            .ok_or_else(|| DeviceError::UnknownDevice(device_id.to_string()))
    }

    pub fn get(&self, device_id: &str) -> DeviceResult<&DeviceRecord> {
        self.devices
            .get(device_id)
            .ok_or_else(|| DeviceError::UnknownDevice(device_id.to_string()))
    }

    pub fn get_mut(&mut self, device_id: &str) -> DeviceResult<&mut DeviceRecord> {
        self.devices
            .get_mut(device_id)
            .ok_or_else(|| DeviceError::UnknownDevice(device_id.to_string()))
    }

    pub fn contains(&self, device_id: &str) -> bool {
        self.devices.contains_key(device_id)
    }

    pub fn require_active(&self, device_id: &str) -> DeviceResult<&DeviceRecord> {
        let device = self.get(device_id)?;

        match device.status {
            DeviceStatus::Active => Ok(device),
            DeviceStatus::Pending => Err(DeviceError::DevicePending {
                device_id: device_id.to_string(),
            }),
            DeviceStatus::Revoked => Err(DeviceError::DeviceRevoked {
                device_id: device_id.to_string(),
            }),
        }
    }

    pub fn all(&self) -> impl Iterator<Item = &DeviceRecord> {
        self.devices.values()
    }

    pub fn pending(&self) -> impl Iterator<Item = &DeviceRecord> {
        self.devices.values().filter(|device| device.is_pending())
    }

    pub fn active(&self) -> impl Iterator<Item = &DeviceRecord> {
        self.devices.values().filter(|device| device.is_active())
    }

    pub fn revoked(&self) -> impl Iterator<Item = &DeviceRecord> {
        self.devices.values().filter(|device| device.is_revoked())
    }

    pub fn len(&self) -> usize {
        self.devices.len()
    }

    pub fn is_empty(&self) -> bool {
        self.devices.is_empty()
    }
}
