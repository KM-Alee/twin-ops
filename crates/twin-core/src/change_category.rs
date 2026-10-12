use serde::Serialize;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeCategory {
    ProcessStarted,
    ProcessDisappeared,
    ServiceRestarted,
    ConfigChanged,
    ListeningPortAdded,
    ListeningPortDisappeared,
    OutboundEndpointAdded,
    ContainerAdded,
    ImageChanged,
    PackageVersionChanged,
    KubernetesObjectChanged,
}

impl fmt::Display for ChangeCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::ProcessStarted => "new processes",
            Self::ProcessDisappeared => "disappeared processes",
            Self::ServiceRestarted => "likely restarted services",
            Self::ConfigChanged => "changed configs",
            Self::ListeningPortAdded => "new listening ports",
            Self::ListeningPortDisappeared => "disappeared listening ports",
            Self::OutboundEndpointAdded => "new outbound endpoints",
            Self::ContainerAdded => "new containers",
            Self::ImageChanged => "changed images",
            Self::PackageVersionChanged => "changed package versions",
            Self::KubernetesObjectChanged => "changed Kubernetes objects",
        })
    }
}

impl std::str::FromStr for ChangeCategory {
    type Err = crate::ParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "process_started" => Ok(Self::ProcessStarted),
            "process_disappeared" => Ok(Self::ProcessDisappeared),
            "service_restarted" => Ok(Self::ServiceRestarted),
            "config_changed" => Ok(Self::ConfigChanged),
            "listening_port_added" => Ok(Self::ListeningPortAdded),
            "listening_port_disappeared" => Ok(Self::ListeningPortDisappeared),
            "outbound_endpoint_added" => Ok(Self::OutboundEndpointAdded),
            "container_added" => Ok(Self::ContainerAdded),
            "image_changed" => Ok(Self::ImageChanged),
            "package_version_changed" => Ok(Self::PackageVersionChanged),
            "kubernetes_object_changed" => Ok(Self::KubernetesObjectChanged),
            _ => Err(crate::ParseError::Enum {
                kind: "change category",
                value: value.into(),
            }),
        }
    }
}
