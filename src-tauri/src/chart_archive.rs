use anyhow::{bail, Context, Result};
use std::{io::Cursor, path::Path};
use zip::{write::FileOptions, CompressionMethod, ZipWriter};

/// Upload directories with exactly the same relative paths the local renderer
/// reads. Mobile document-tree imports are directories, not archive files.
pub(crate) fn read_upload(path: &Path) -> Result<(String, Vec<u8>)> {
    if !path.is_dir() {
        let name = path.file_name().and_then(|value| value.to_str()).unwrap_or("chart.pez");
        return Ok((name.to_owned(), std::fs::read(path)?));
    }
    fn append(writer: &mut ZipWriter<Cursor<Vec<u8>>>, root: &Path, directory: &Path) -> Result<()> {
        let options = FileOptions::default().compression_method(CompressionMethod::Stored);
        for entry in std::fs::read_dir(directory)? {
            let entry = entry?;
            let path = entry.path();
            let relative = path.strip_prefix(root)?;
            let name = relative.to_str().context("The chart contains a non-Unicode filename")?
                .replace(std::path::MAIN_SEPARATOR, "/");
            let kind = entry.file_type()?;
            if kind.is_dir() {
                writer.add_directory(format!("{name}/"), options)?;
                append(writer, root, &path)?;
            } else if kind.is_file() {
                writer.start_file(name, options)?;
                std::io::copy(&mut std::fs::File::open(&path)?, writer)?;
            } else {
                bail!("Cannot upload special chart file: {}", path.display());
            }
        }
        Ok(())
    }
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    append(&mut writer, path, path)?;
    Ok(("chart.zip".to_owned(), writer.finish()?.into_inner()))
}
