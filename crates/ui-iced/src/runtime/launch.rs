use std::ffi::OsString;
use std::io;
use std::path::PathBuf;

#[derive(Clone)]
pub struct Launch {
    pub home: PathBuf,
    pub directory: Option<PathBuf>,
    pub state_dir: PathBuf,
}

impl Launch {
    pub fn from_env() -> io::Result<Self> {
        let home = std::env::var_os("HOME").map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Cannot start browsing: HOME is not an absolute folder path. Set HOME and relaunch."))?;
        Self::parse(std::env::args_os().skip(1), home, std::env::current_dir()?)
    }

    pub(crate) fn parse(
        mut arguments: impl Iterator<Item = OsString>,
        home: PathBuf,
        current: PathBuf,
    ) -> io::Result<Self> {
        let mut directory = None;
        let mut state_dir = None;
        while let Some(option) = arguments.next() {
            let target = if option == "--directory" {
                &mut directory
            } else if option == "--state-dir" {
                &mut state_dir
            } else {
                return Err(argument_error());
            };
            if target.is_some() {
                return Err(argument_error());
            }
            let path = arguments
                .next()
                .filter(|path| !path.is_empty())
                .map(PathBuf::from)
                .ok_or_else(argument_error)?;
            if path.as_os_str() == "--directory" || path.as_os_str() == "--state-dir" {
                return Err(argument_error());
            }
            *target = Some(if path.is_absolute() {
                path
            } else {
                current.join(path)
            });
        }
        Ok(Self {
            directory,
            state_dir: state_dir.unwrap_or_else(|| crate::platform::state_directory(&home)),
            home,
        })
    }
}

fn argument_error() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "Cannot launch: unsupported, repeated, or incomplete arguments. Use --directory <folder> and/or --state-dir <private-root>, or launch without arguments for automatic workspace saving.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_automatically_selects_storage_and_preserves_explicit_overrides() -> io::Result<()> {
        let home = PathBuf::from("/Users/fixture");
        let current = PathBuf::from("/fixture");
        assert_eq!(
            Launch::parse([].into_iter(), home.clone(), current.clone())?.state_dir,
            home.join("Library/Application Support/file-tile")
        );
        for arguments in [
            ["--directory", "A B", "--state-dir", "State"],
            ["--state-dir", "State", "--directory", "A B"],
        ] {
            let launch = Launch::parse(
                arguments.map(OsString::from).into_iter(),
                home.clone(),
                current.clone(),
            )?;
            assert_eq!(launch.state_dir, current.join("State"));
            assert_eq!(launch.directory, Some(current.join("A B")));
        }
        for arguments in [
            vec!["--state-dir"],
            vec!["--state-dir", ""],
            vec!["--state-dir", "--directory"],
            vec!["--state-dir", "A", "--state-dir", "B"],
        ] {
            assert!(
                Launch::parse(
                    arguments.into_iter().map(OsString::from),
                    home.clone(),
                    current.clone()
                )
                .is_err()
            );
        }
        Ok(())
    }
}
