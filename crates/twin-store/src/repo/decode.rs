use crate::StoreError;

pub(crate) fn parse_field<T, E, F>(field: &str, value: &str, parse: F) -> Result<T, StoreError>
where
    E: std::fmt::Display,
    F: FnOnce(&str) -> Result<T, E>,
{
    parse(value).map_err(|e| StoreError::Decode {
        detail: format!("invalid {field} `{value}`: {e}"),
    })
}
