use crate::json_utils::JsonValue;
use crate::m_try;
use crate::registry::ETypesRegistry;
use crate::value::id::ETypeId;
use crate::value::EValue;
use camino::{Utf8Path, Utf8PathBuf};
use miette::{miette, Context, IntoDiagnostic, Report};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Debug)]
pub struct Project {
    pub registry: ETypesRegistry,
    pub files: BTreeMap<Utf8PathBuf, ProjectFile>,
    pub root: Utf8PathBuf,
    pub config: Config,
}

#[derive(Debug)]
pub enum ProjectFile {
    /// Valid plain JSON value
    Value(EValue),
    /// Plain JSON value that had issues during parsing or loading
    BadValue(Report),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    #[serde(rename = "types")]
    types_config: TypesConfig,
}

#[derive(Debug, Serialize, Deserialize)]
struct TypesConfig {
    root: String,
    import: ETypeId,
}

impl Project {
    pub fn from_files(
        root: impl AsRef<Path>,
        config: Config,
        files: impl IntoIterator<Item = PathBuf>,
        read_file: impl Fn(&Path) -> miette::Result<Vec<u8>>,
    ) -> miette::Result<Self> {
        let mut registry_items = BTreeMap::new();
        let mut jsons = BTreeMap::<Utf8PathBuf, JsonValue>::new();

        fn utf8str(path: &Utf8Path, data: Vec<u8>) -> miette::Result<String> {
            String::from_utf8(data).into_diagnostic().with_context(|| {
                format!(
                    "failed to parse content of a file `{path}`. Are you sure it's UTF-8 encoded?"
                )
            })
        }

        let root = root.as_ref();
        let root = Utf8PathBuf::from_path_buf(root.to_path_buf())
            .map_err(|_| miette!("Got non-UTF8 path at {}", root.display()))?;

        for path in files {
            let relative = path
                .strip_prefix(&root).map_err(|_|miette!("directory contains file `{}` which is outside of the directory. Are there symlinks?", path.display()))?;

            let path = Utf8Path::from_path(relative)
                .ok_or_else(|| miette!("Got non-UTF8 path at {}", relative.display()))?;

            let Some(ext) = path.extension() else {
                continue;
            };

            m_try(|| {
                match ext {
                    "kdl" => {
                        let id = ETypeId::from_path(path, &config.types_config.root)
                            .context("failed to generate type identifier")?;
                        let value = utf8str(path, read_file(path.as_ref())?)?;
                        registry_items.insert(id, value);
                    }
                    "json5" | "json" => {
                        let data =
                            serde_json5::from_str(&utf8str(path, read_file(path.as_ref())?)?)
                                .into_diagnostic()
                                .context("failed to deserialize JSON")?;
                        jsons.insert(path.to_path_buf(), data);
                    }
                    _ => {}
                }

                Ok(())
            })
            .with_context(|| format!("failed to load file at `{}`", path))?;
        }

        let registry = ETypesRegistry::from_raws(registry_items)?;

        let mut project = Self {
            registry,
            files: Default::default(),
            root,
            config,
        };

        project.validate_config()?;

        for (path, json) in jsons {
            let item = match project
                .deserialize_json(json)
                .with_context(|| format!("failed to deserialize JSON at `{}`", path))
            {
                Ok(data) => ProjectFile::Value(data),
                Err(err) => ProjectFile::BadValue(err),
            };
            project.files.insert(path, item);
        }

        Ok(project)
    }

    pub fn from_path(root: impl AsRef<Path>) -> miette::Result<Self> {
        let root = root.as_ref();

        let mut paths = BTreeSet::new();
        let wd = WalkDir::new(root);
        for entry in wd {
            let entry = entry.into_diagnostic()?;
            if entry.path().is_dir() {
                continue;
            }

            paths.insert(entry.path().to_path_buf());
        }

        let config = fs_err::read_to_string(root.join("project.toml"))
            .into_diagnostic()
            .context("failed to read project configuration")?;

        let config = toml::de::from_str(&config)
            .into_diagnostic()
            .context("Failed to parse project configuration")?;

        // let items = paths
        //     .into_par_iter()
        //     .map(|path| {
        //         let data = fs_err::read(&path).into_diagnostic()?;
        //         miette::Result::<(PathBuf, Vec<u8>)>::Ok((path, data))
        //     })
        //     .collect::<Result<Vec<_>, _>>()?;

        Self::from_files(root, config, paths, |path| {
            fs_err::read(root.join(path)).into_diagnostic()
        })
    }
}

impl Project {
    // pub fn get_value(&mut self, path: &Utf8Path) -> Option<&mut ProjectFile> {
    //     self.files.get_mut(path)
    // }

    fn validate_config(&self) -> miette::Result<()> {
        self.registry
            .get_object(&self.config.types_config.import)
            .ok_or_else(|| miette!("unknown type `{}`", self.config.types_config.import))
            .context("failed to validate [types.import] config entry")
            .context("project config is invalid")?;

        Ok(())
    }

    fn deserialize_json(&self, mut value: JsonValue) -> miette::Result<EValue> {
        let object = self
            .registry
            .get_object(&self.config.types_config.import)
            .expect("Config was validated");

        object.parse_json(&self.registry, &mut value, false)
    }
}