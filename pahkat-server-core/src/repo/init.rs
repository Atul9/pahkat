use std::io::{self, Read, Write};
use std::fs::{self, File, create_dir_all};
use std::path::{Path, PathBuf};
use std::borrow::Cow;

use serde::Serialize;
use typed_builder::TypedBuilder;
use url::Url;

use pahkat_types::{package::Index as PackagesIndex, repo::{Repository, Index, Agent}};

#[non_exhaustive]
#[derive(Debug, Clone, TypedBuilder)]
pub struct InitRequest<'a> {
    pub path: Cow<'a, Path>,
    pub base_url: Cow<'a, Url>,
}

#[non_exhaustive]
#[derive(Debug, Clone, Default, TypedBuilder)]
pub struct PartialInitRequest<'a> {
    #[builder(default)]
    pub path: Option<&'a Path>,
    #[builder(default)]
    pub base_url: Option<&'a Url>,
}

#[derive(Debug, thiserror::Error)]
pub enum RequestError {
    #[error("Provided path was invalid")]
    PathError(#[source] io::Error),

    #[error("Invalid input")]
    InvalidInput,

    #[error("Invalid URL")]
    InvalidUrl(#[from] url::ParseError),
}

impl<'a> crate::Request for InitRequest<'a> {
    type Error = RequestError;
    type Partial = PartialInitRequest<'a>;

    fn new_from_user_input(partial: Self::Partial) -> Result<Self, Self::Error> {
        use dialoguer::{Input};

        let path = match partial.path {
            Some(path) => Cow::Borrowed(path),
            None => {
                Input::<String>::new()
                    .default(std::env::current_dir().ok().and_then(|x| x.to_str().map(str::to_string)).unwrap_or_else(|| ".".into()))
                    .with_prompt("Path")
                    .interact()
                    .map(|p| Cow::Owned(PathBuf::from(p)))
                    .map_err(RequestError::PathError)?
            }
        };

        let base_url = match partial.base_url {
            Some(url) => Cow::Borrowed(url),
            None => {
                let base_url = Input::<String>::new().with_prompt("Base URL")
                    .interact()
                    .map_err(|_| RequestError::InvalidInput)?;
                Cow::Owned(Url::parse(&base_url)?)
            }
        };

        Ok(InitRequest { path, base_url })
    }
}

pub fn create_agent() -> Agent {
    Agent {
        name: "pahkat".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        url: Some(Url::parse("https://github.com/divvun/pahkat/").unwrap())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Failed to create directory `{0}`")]
    DirCreateFailed(PathBuf, #[source] io::Error),

    #[error("Failed to write TOML file `{0}`")]
    WriteToml(PathBuf, #[source] io::Error),

    #[error("Failed to serialize TOML for `{0}`")]
    SerializeToml(PathBuf, #[source] toml::ser::Error),
}

fn write_index<T: Serialize>(path: &Path, index: &T) -> Result<(), Error> {
    let data = toml::to_string(index).map_err(|e| Error::SerializeToml(path.to_path_buf(), e))?;
    fs::write(&path, data).map_err(|e| Error::WriteToml(path.to_path_buf(), e))
}

pub fn init<'a>(request: InitRequest<'a>) -> Result<(), Error> {
    // Create all the directories
    create_dir_all(&request.path)
        .map_err(|e| Error::DirCreateFailed(request.path.to_path_buf(), e))?;
    create_dir_all(&request.path.join("packages"))
        .map_err(|e| Error::DirCreateFailed(request.path.to_path_buf(), e))?;

    // Create empty repository index
    let index = Index::builder()
        .base_url(request.base_url.into_owned())
        .agent(create_agent())
        .build();

    let repo_index_path = request.path.join("index.toml");
    write_index(&repo_index_path, &index)?;

    // Create empty packages index
    let packages_index = PackagesIndex::builder().build();
    let packages_index_path = request.path.join("packages/index.toml");
    write_index(&packages_index_path, &packages_index)?;

    Ok(())
}