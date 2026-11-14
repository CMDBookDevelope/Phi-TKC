use std::path::PathBuf;

const REQUIRED_ASSETS: &[&str] = &["font.ttf", "ending.mp3", "respack/info.yml"];

/// Select the first directory containing the renderer's required resources.
pub(crate) fn find_renderer_assets(
    candidates: impl IntoIterator<Item = PathBuf>,
) -> Option<PathBuf> {
    candidates.into_iter().find(|directory| {
        REQUIRED_ASSETS
            .iter()
            .all(|asset| directory.join(asset).is_file())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        path::Path,
        sync::atomic::{AtomicUsize, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "phi-tkc-assets-{}-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed),
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn write_assets(directory: &Path) {
        for asset in REQUIRED_ASSETS {
            let path = directory.join(asset);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, asset.as_bytes()).unwrap();
        }
    }

    #[test]
    fn ios_nested_resources_skip_the_outer_assets_container() {
        let temporary = TestDirectory::new();
        let container = temporary.0.join("Phi-TKC.app/assets");
        let resources = container.join("assets");
        write_assets(&resources);
        assert!(container.is_dir());
        let selected = find_renderer_assets([container, resources.clone()]).unwrap();
        assert_eq!(selected, resources);
        for asset in REQUIRED_ASSETS {
            assert_eq!(fs::read(selected.join(asset)).unwrap(), asset.as_bytes());
        }
    }

    #[test]
    fn flat_ios_resources_are_still_supported() {
        let temporary = TestDirectory::new();
        let resources = temporary.0.join("Phi-TKC.app/assets");
        write_assets(&resources);
        assert_eq!(
            find_renderer_assets([resources.join("assets"), resources.clone()]),
            Some(resources),
        );
    }

    #[test]
    fn each_missing_required_asset_rejects_a_directory() {
        let temporary = TestDirectory::new();
        for (index, asset) in REQUIRED_ASSETS.iter().enumerate() {
            let resources = temporary.0.join(index.to_string());
            write_assets(&resources);
            fs::remove_file(resources.join(asset)).unwrap();
            assert_eq!(find_renderer_assets([resources]), None, "missing {asset}");
        }
    }

    #[test]
    fn directories_named_after_required_files_are_rejected() {
        let temporary = TestDirectory::new();
        for (index, asset) in REQUIRED_ASSETS.iter().enumerate() {
            let resources = temporary.0.join(index.to_string());
            write_assets(&resources);
            let impostor = resources.join(asset);
            fs::remove_file(&impostor).unwrap();
            fs::create_dir(impostor).unwrap();
            assert_eq!(find_renderer_assets([resources]), None, "directory {asset}");
        }
    }

    #[test]
    fn valid_candidates_keep_the_callers_preference_order() {
        let temporary = TestDirectory::new();
        let first = temporary.0.join("first");
        let second = temporary.0.join("second");
        write_assets(&first);
        write_assets(&second);
        assert_eq!(
            find_renderer_assets([first.clone(), second.clone()]),
            Some(first.clone()),
        );
        assert_eq!(find_renderer_assets([second.clone(), first]), Some(second));
    }

    #[test]
    fn no_candidates_return_no_resource_directory() {
        assert_eq!(find_renderer_assets([]), None);
    }
}
