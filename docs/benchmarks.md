# Local sync benchmarks

Measured on 2026-09-11 with `rustc 1.96.1 (31fca3adb 2026-06-26)`, release binaries, loopback HTTP and a fresh SQLite store.

System: `Linux 7.0.0-27-generic x86_64`.

Source: `f50b8f1`, with no tracked source changes. [Raw samples](benchmark-samples.jsonl).

CPU: Intel(R) Core(TM) i7-8750H CPU @ 2.20GHz.

Each result is the median of three sequential runs. Timings include CLI process startup, scanning, encryption, HTTP and storage. Build, server startup and fixture generation are excluded. Each small file initially contains 1,024 bytes; MB means 1,000,000 bytes.

| Scenario | Median ms | Uploaded blobs | Uploaded bytes | Downloaded blobs |
| --- | ---: | ---: | ---: | ---: |
| 1 changed file among 1,000 | 222.36 | 1 | 78 | 0 |
| 1,000 small files, no changes | 72.29 | 0 | 0 | 0 |
| 10 MB file, new contents | 962.28 | 10 | 10000550 | 0 |
| Second sync with unchanged data | 205.43 | 0 | 0 | 0 |

Run `scripts/benchmark.sh` to reproduce. The script checks every sample: unchanged syncs transfer zero blobs and do not publish; changing one small file uploads exactly one blob, reusing the other 999 remote files. The 10 MB cases use fresh random data and require multiple uploaded chunks. Transfer counts exclude manifests and HTTP framing. These are local measurements, not network throughput guarantees.
