use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub struct SoldeerError {
    pub message: String,
}

impl fmt::Display for SoldeerError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct MissingDependencies {
    pub name: String,
    pub version: String,
}

impl MissingDependencies {
    pub fn new(name: &str, version: &str) -> MissingDependencies {
        MissingDependencies {
            name: name.to_string(),
            version: version.to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct UnzippingError {
    pub name: String,
    pub version: String,
}

impl UnzippingError {
    pub fn new(name: &str, version: &str) -> UnzippingError {
        UnzippingError {
            name: name.to_string(),
            version: version.to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct IncorrectDependency {
    pub name: String,
    pub version: String,
}

impl IncorrectDependency {
    pub fn new(name: &str, version: &str) -> IncorrectDependency {
        IncorrectDependency {
            name: name.to_string(),
            version: version.to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LockError {
    pub cause: String,
}

impl fmt::Display for LockError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "lock failed")
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DownloadError {
    pub name: String,
    pub version: String,
    pub cause: String,
}

impl fmt::Display for DownloadError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "download failed for {}~{}", &self.name, &self.version)
    }
}

impl DownloadError {
    pub fn new(name: &str, version: &str, cause: &str) -> DownloadError {
        DownloadError {
            name: name.to_string(),
            version: version.to_string(),
            cause: cause.to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProjectNotFound {
    pub name: String,
    pub cause: String,
}

impl ProjectNotFound {
    pub fn new(name: &str, cause: &str) -> ProjectNotFound {
        ProjectNotFound {
            name: name.to_string(),
            cause: cause.to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PushError {
    pub name: String,
    pub version: String,
    pub cause: String,
}

impl PushError {
    pub fn new(name: &str, version: &str, cause: &str) -> PushError {
        PushError {
            name: name.to_string(),
            version: version.to_string(),
            cause: cause.to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LoginError {
    pub cause: String,
}

impl LoginError {
    pub fn new(cause: &str) -> LoginError {
        LoginError {
            cause: cause.to_string(),
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct ConfigError {
    pub cause: String,
}

impl ConfigError {
    pub fn new(cause: &str) -> ConfigError {
        ConfigError {
            cause: cause.to_string(),
        }
    }
}
