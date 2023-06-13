use std::fs::File;

use serde::{Serialize, Deserialize};

fn main() {
    let j = serde_json::from_reader(File::open("target/obj.json").unwrap()).unwrap();
    let mut hashes = vec![];
    traverse(&j, &mut hashes, Default::default());

    println!("{}", hashes.len());
    for (path, hash) in hashes {
        let path = serde_json::to_string(&path).unwrap();
        let Ok(mut bytes) = bs58::decode(&hash).with_check(Some(16)).into_vec() else {
            continue;
        };
        bytes.reverse();
        let h = hex::encode(&bytes[..(bytes.len() - 2)]);
        println!("{h}, {hash}, {path}");
    }

    #[derive(Default, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
    struct JsonPath(Vec<String>);

    fn traverse(j: &serde_json::Value, hashes: &mut Vec<(JsonPath, String)>, path: JsonPath) {
        match j {
            serde_json::Value::Object(obj) => {
                for (k, v) in obj {
                    let mut path = path.clone();
                    path.0.push(k.clone());
                    traverse(v, hashes, path);
                }
            }
            serde_json::Value::Array(obj) => {
                for (i, v) in obj.iter().enumerate() {
                    let mut path = path.clone();
                    path.0.push(i.to_string());
                    traverse(v, hashes, path);
                }
            }
            serde_json::Value::String(str) => {
                if str.starts_with('3') && str.len() >= 50 {
                    hashes.push((path, str.clone()));
                }
            }
            _ => (),
        }
    }
}
