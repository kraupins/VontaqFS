#!/usr/bin/env python3
"""CP15 independent source contract review; not native execution."""
from pathlib import Path
src = (Path(__file__).resolve().parents[1] / 'crates/core/src/storage.rs').read_text()
read = src.split('pub fn read_small_file', 1) if 'pub fn read_small_file' in src else []
export = src.split('NativeExportCapture::create(self, &data_root, &selected, &is_cancelled)?;', 1)
assert len(export) == 2
# A captured private source tree survives outside short-lived source leases.
assert export[1].index('drop(source_lease);') < export[1].index('SimpleZipWriter::new(')
assert 'captured_sources.root.as_path()' in export[1].split('SimpleZipWriter::new(', 1)[0]
backup = src.split('pub fn create_portable_backup', 1)[1].split('pub fn restore_portable_backup', 1)[0]
assert backup.index('NativeExportCapture::create(') < backup.index('drop(capture_lease)') < backup.index('SimpleZipWriter::new(')
assert 'captured_sources.root.as_path()' in backup
assert 'zip.add_file_verified_cancellable(' in backup
zip_impl = src.split('impl<W: Write + Seek> SimpleZipWriter<W> {', 1)[1].split('fn finish', 1)[0]
assert 'let mut observed_crc = 0xffff_ffffu32' in zip_impl
assert 'observed_sha.update(&buffer[..read]);' in zip_impl
assert 'self.inner.write_all(&buffer[..read])?;' in zip_impl
assert 'if written != size' in zip_impl
assert 'observed != expected' in zip_impl
assert 'write_u64(&mut self.inner, size)?;' in zip_impl
assert 'self.inner.seek(SeekFrom::Start(offset + 14))?;' in zip_impl
finish = src.split('    fn finish(mut self) -> StorageResult<()> {', 1)[1]
assert 'write_u32(&mut self.inner, 0x06064b50)?;' in finish  # ZIP64 EOCD
assert 'write_u32(&mut self.inner, 0x07064b50)?;' in finish  # ZIP64 locator
assert 'write_u32(&mut self.inner, 0x06054b50)?;' in finish  # ordinary EOCD
print('PASS CP15 source: immutable captures + single-pass SHA/CRC + ZIP64 markers (native NOT RUN)')
