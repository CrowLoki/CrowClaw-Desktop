use std::{
    ffi::OsString,
    path::{Component, Path, PathBuf},
};

#[derive(Debug, Default, PartialEq, Eq)]
pub struct LaunchOptions {
    pub profile_directory: Option<PathBuf>,
}

impl LaunchOptions {
    /// An alternate profile is explicit, local and absolute. Invalid arguments
    /// never silently fall back to the user's default data directory.
    pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Self, String> {
        let mut args = args.into_iter();
        let mut options = Self::default();
        while let Some(arg) = args.next() {
            if arg != "--profile-dir" {
                return Err("Unknown startup option. Use --profile-dir followed by an absolute local directory.".into());
            }
            if options.profile_directory.is_some() {
                return Err("Specify --profile-dir only once".into());
            }
            let path = PathBuf::from(args.next().ok_or("--profile-dir requires a directory")?);
            validate_profile_path(&path)?;
            options.profile_directory = Some(path);
        }
        Ok(options)
    }
}

fn validate_profile_path(path: &Path) -> Result<(), String> {
    if !path.is_absolute()
        || path.parent().is_none()
        || path.file_name().is_none()
        || path.components().any(|c| matches!(c, Component::ParentDir))
    {
        return Err("A profile must be an absolute local directory below a drive root, without parent traversal".into());
    }
    #[cfg(windows)]
    if !matches!(path.components().next(),Some(Component::Prefix(prefix)) if matches!(prefix.kind(),std::path::Prefix::Disk(_)|std::path::Prefix::VerbatimDisk(_)))
    {
        return Err("A profile must use a local drive, not a network share or device path".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn default_launch_preserves_the_default_profile() {
        assert_eq!(
            LaunchOptions::parse(Vec::new()).unwrap(),
            LaunchOptions::default()
        );
    }
    #[test]
    fn explicit_profile_is_kept_without_filesystem_changes() {
        let dir = tempfile::tempdir().unwrap();
        let selected = dir.path().join("new profile");
        let parsed = LaunchOptions::parse([
            OsString::from("--profile-dir"),
            selected.clone().into_os_string(),
        ])
        .unwrap();
        assert_eq!(parsed.profile_directory, Some(selected.clone()));
        assert!(!selected.exists());
    }
    #[test]
    fn invalid_or_ambiguous_profiles_never_fall_back() {
        assert!(LaunchOptions::parse([OsString::from("--profile-dir")]).is_err());
        assert!(LaunchOptions::parse([
            OsString::from("--profile-dir"),
            OsString::from("relative")
        ])
        .is_err());
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profile").into_os_string();
        assert!(LaunchOptions::parse([
            OsString::from("--profile-dir"),
            path.clone(),
            OsString::from("--profile-dir"),
            path
        ])
        .is_err());
        assert!(LaunchOptions::parse([OsString::from("--unknown")]).is_err());
    }
    #[cfg(windows)]
    #[test]
    fn rejects_root_traversal_and_unc_profile_targets() {
        for path in [
            "C:\\",
            "C:\\profile\\..\\elsewhere",
            "\\\\server\\profile",
            "\\\\.\\C:\\profile",
        ] {
            assert!(
                LaunchOptions::parse([OsString::from("--profile-dir"), OsString::from(path)])
                    .is_err(),
                "{path}"
            );
        }
    }
}
