#[derive(Clone, PartialEq, prost::Message)]
pub(crate) struct Empty {}

#[derive(Clone, PartialEq, prost::Message)]
pub(crate) struct Namespace {
    #[prost(string, tag = "1")]
    pub name: String,
}

#[derive(Clone, PartialEq, prost::Message)]
pub(crate) struct Namespaces {
    #[prost(message, repeated, tag = "1")]
    pub namespaces: Vec<Namespace>,
}

#[derive(Clone, PartialEq, prost::Message)]
pub(crate) struct Timestamp {
    #[prost(int64, tag = "1")]
    pub seconds: i64,
    #[prost(int32, tag = "2")]
    pub nanos: i32,
}

#[derive(Clone, PartialEq, prost::Message)]
pub(crate) struct Container {
    #[prost(string, tag = "1")]
    pub id: String,
    #[prost(string, tag = "3")]
    pub image: String,
    #[prost(message, optional, tag = "8")]
    pub created_at: Option<Timestamp>,
}

#[derive(Clone, PartialEq, prost::Message)]
pub(crate) struct Containers {
    #[prost(message, repeated, tag = "1")]
    pub containers: Vec<Container>,
}

#[derive(Clone, PartialEq, prost::Message)]
pub(crate) struct Task {
    #[prost(string, tag = "1")]
    pub container_id: String,
    #[prost(uint32, tag = "3")]
    pub pid: u32,
    #[prost(int32, tag = "4")]
    pub status: i32,
}

#[derive(Clone, PartialEq, prost::Message)]
pub(crate) struct Tasks {
    #[prost(message, repeated, tag = "1")]
    pub tasks: Vec<Task>,
}

#[derive(Clone, PartialEq, prost::Message)]
pub(crate) struct Descriptor {
    #[prost(string, tag = "2")]
    pub digest: String,
}

#[derive(Clone, PartialEq, prost::Message)]
pub(crate) struct Image {
    #[prost(string, tag = "1")]
    pub name: String,
    #[prost(message, optional, tag = "3")]
    pub target: Option<Descriptor>,
}

#[derive(Clone, PartialEq, prost::Message)]
pub(crate) struct Images {
    #[prost(message, repeated, tag = "1")]
    pub images: Vec<Image>,
}

#[cfg(test)]
mod tests {
    use super::{Container, Task};
    use prost::Message;

    #[test]
    fn projections_skip_oci_spec_labels_and_task_io() {
        let mut bytes = Container {
            id: "api".into(),
            image: "example/api:1".into(),
            created_at: None,
        }
        .encode_to_vec();
        for tag in [2u8, 5] {
            let sentinel = b"PRIVATE_VALUE";
            bytes.extend_from_slice(&[tag << 3 | 2, sentinel.len() as u8]);
            bytes.extend_from_slice(sentinel);
        }
        let projected = Container::decode(bytes.as_slice()).unwrap();
        assert_eq!(projected.id, "api");
        assert!(!projected
            .encode_to_vec()
            .windows(13)
            .any(|bytes| bytes == b"PRIVATE_VALUE"));
        let mut bytes = Task {
            container_id: "api".into(),
            pid: 42,
            status: 2,
        }
        .encode_to_vec();
        bytes.extend_from_slice(&[5 << 3 | 2, 13]);
        bytes.extend_from_slice(b"PRIVATE_VALUE");
        let projected = Task::decode(bytes.as_slice()).unwrap();
        assert_eq!(projected.pid, 42);
        assert_eq!(projected.status, 2);
        assert!(!projected
            .encode_to_vec()
            .windows(13)
            .any(|bytes| bytes == b"PRIVATE_VALUE"));
    }
}
