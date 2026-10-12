use std::fmt;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageSource {
    Dpkg,
    Pacman,
    Rpm,
}
impl fmt::Display for PackageSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Dpkg => "dpkg",
            Self::Pacman => "pacman",
            Self::Rpm => "rpm",
        })
    }
}
