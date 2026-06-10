//! ALAN Echo — JSON-backed persistent settings.

use serde_json::{Map, Value};
use std::path::{Path, PathBuf};

pub struct Settings {
    path: PathBuf,
    data: Map<String, Value>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            path: PathBuf::new(),
            data: Map::new(),
        }
    }
}

impl Settings {
    pub fn load(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let data = if path.exists() {
            let content = std::fs::read_to_string(path)?;
            match serde_json::from_str::<Value>(&content)? {
                Value::Object(map) => map,
                _ => Map::new(),
            }
        } else {
            Map::new()
        };
        Ok(Self { path: path.to_path_buf(), data })
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.data.get(key)
    }

    pub fn get_str(&self, key: &str) -> Option<String> {
        self.data.get(key).and_then(|v| v.as_str()).map(|s| s.to_string())
    }

    pub fn get_bool(&self, key: &str) -> Option<bool> {
        self.data.get(key).and_then(|v| v.as_bool())
    }

    pub fn set(&mut self, key: &str, value: Value) {
        self.data.insert(key.to_string(), value);
    }

    pub fn delete(&mut self, key: &str) {
        self.data.remove(key);
    }

    pub fn to_json(&self) -> Value {
        Value::Object(self.data.clone())
    }

    pub fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
        let content = serde_json::to_string_pretty(&self.data)?;
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, &content)?;
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }
}
