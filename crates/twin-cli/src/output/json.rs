pub fn render<T: serde::Serialize + ?Sized>(value: &T) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(value)
}
