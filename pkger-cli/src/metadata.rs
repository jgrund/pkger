use pkger_core::recipe::{BuildArch, BuildTarget};
use pkger_core::{ErrContext, Result};

use lazy_static::lazy_static;
use regex::Regex;
use std::convert::TryFrom;
use std::fs::DirEntry;
use std::time::SystemTime;

lazy_static! {
    static ref DEB_RE: Regex = Regex::new(r"([\w.-]+?)-(\d+[.]\d+[.]\d+)[.]([\w_-]+)").unwrap();
    static ref RPM_RE: Regex =
        Regex::new(r"([\w_.-]+?)-(\d+[.]\d+[.]\d+)-(\d+)[.]([\w_-]+)").unwrap();
    static ref PKG_RE: Regex =
        Regex::new(r"([\w_.+@-]+?)-(\d+[.]\d+[.]\d+)-(\d+)-([\w_-]+)").unwrap();
    static ref GZIP_RE: Regex = Regex::new(r"([\S]+?)-(\d+[.]\d+[.]\d+)").unwrap();
}

#[derive(Debug, PartialEq)]
pub struct PackageMetadata {
    name: String,
    version: String,
    release: Option<String>,
    arch: Option<BuildArch>,
    package_type: BuildTarget,
    created: Option<SystemTime>,
}

impl PackageMetadata {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn release(&self) -> &Option<String> {
        &self.release
    }

    pub fn arch(&self) -> &Option<BuildArch> {
        &self.arch
    }

    pub fn package_type(&self) -> BuildTarget {
        self.package_type
    }

    pub fn created(&self) -> Option<SystemTime> {
        self.created
    }

    pub fn try_from_dir_entry(e: &DirEntry) -> Result<Self> {
        let path = e.path();
        let extension = path.extension().context("expected file extension")?;
        let package_type = BuildTarget::try_from(extension.to_string_lossy().as_ref())?;
        let path = path
            .file_stem()
            .context("expected a file name")?
            .to_string_lossy();
        let path = path.as_ref();

        Self::try_from_str(
            path,
            package_type,
            e.metadata().and_then(|md| md.created()).ok(),
        )
        .context("invalid package name, the name did not match any scheme")
    }

    fn try_from_str(
        s: &str,
        package_type: BuildTarget,
        created: Option<SystemTime>,
    ) -> Option<Self> {
        match package_type {
            BuildTarget::Deb => DEB_RE
                .captures_iter(s)
                .next()
                .map(|captures| PackageMetadata {
                    name: captures[1].to_string(),
                    version: captures[2].to_string(),
                    release: None,
                    arch: BuildArch::try_from(&captures[3]).ok(),
                    package_type,
                    created,
                }),
            BuildTarget::Rpm => RPM_RE
                .captures_iter(s)
                .next()
                .map(|captures| PackageMetadata {
                    name: captures[1].to_string(),
                    version: captures[2].to_string(),
                    release: Some(captures[3].to_string()),
                    arch: BuildArch::try_from(&captures[4]).ok(),
                    package_type,
                    created,
                }),
            BuildTarget::Pkg => PKG_RE
                .captures_iter(s)
                .next()
                .map(|captures| PackageMetadata {
                    name: captures[1].to_string(),
                    version: captures[2].to_string(),
                    release: Some(captures[3].to_string()),
                    arch: BuildArch::try_from(&captures[4]).ok(),
                    package_type,
                    created,
                }),
            BuildTarget::Gzip => GZIP_RE
                .captures_iter(s)
                .next()
                .map(|captures| PackageMetadata {
                    name: captures[1].to_string(),
                    version: captures[2].to_string(),
                    release: None,
                    arch: None,
                    package_type,
                    created,
                }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::PackageMetadata;
    use pkger_core::recipe::{BuildArch, BuildTarget};
    use std::time::SystemTime;

    #[test]
    fn parses_deb() {
        let path = "test-instantclient-19.10-basic-1.0.0.amd64";

        assert_eq!(
            PackageMetadata {
                name: "test-instantclient-19.10-basic".to_string(),
                version: "1.0.0".to_string(),
                release: None,
                arch: Some(BuildArch::x86_64),
                package_type: BuildTarget::Deb,
                created: None,
            },
            PackageMetadata::try_from_str(path, BuildTarget::Deb, None).unwrap(),
        );
    }

    #[test]
    fn parses_rpm() {
        let path = "tst-dev-tools-1.0.1-0.x86_64";

        let time = SystemTime::now();

        assert_eq!(
            PackageMetadata {
                name: "tst-dev-tools".to_string(),
                version: "1.0.1".to_string(),
                release: Some("0".to_string()),
                arch: Some(BuildArch::x86_64),
                package_type: BuildTarget::Rpm,
                created: Some(time),
            },
            PackageMetadata::try_from_str(path, BuildTarget::Rpm, Some(time)).unwrap(),
        );
    }

    #[test]
    fn parses_gzip() {
        let path = "tst-dev-tools-1.0.1";

        assert_eq!(
            PackageMetadata {
                name: "tst-dev-tools".to_string(),
                version: "1.0.1".to_string(),
                release: None,
                arch: None,
                package_type: BuildTarget::Gzip,
                created: None,
            },
            PackageMetadata::try_from_str(path, BuildTarget::Gzip, None).unwrap(),
        );
    }

    #[test]
    fn parses_pkg() {
        let path = "pkger-0.5.0-0-x86_64";

        assert_eq!(
            PackageMetadata {
                name: "pkger".to_string(),
                version: "0.5.0".to_string(),
                release: Some("0".to_string()),
                arch: Some(BuildArch::x86_64),
                package_type: BuildTarget::Pkg,
                created: None,
            },
            PackageMetadata::try_from_str(path, BuildTarget::Pkg, None).unwrap(),
        );
    }
}
