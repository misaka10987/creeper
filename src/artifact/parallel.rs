use std::{
    collections::HashMap,
    hash::Hash,
    path::{Path, PathBuf},
};

use futures::{StreamExt, TryStreamExt, stream};
use tracing::debug;

use crate::{Artifact, Checksum, Creeper};

impl Creeper {
    /// Parallel retrieve artifacts and create soft links.
    /// Each artifact is keyed by its relative path under the base path.
    ///
    /// See [`Self::retrieve_artifact_to`] for details and caveats.
    pub async fn batch_retrieve_artifact_to(
        &self,
        map: HashMap<PathBuf, Artifact>,
        base: impl AsRef<Path>,
    ) -> anyhow::Result<()> {
        let base = base.as_ref();

        let count = stream::iter(map)
            .map(
                |(path, art)| async move { self.retrieve_artifact_to(&art, base.join(path)).await },
            )
            .buffer_unordered(self.config.parallel_download)
            .try_collect::<Vec<_>>()
            .await?
            .len();

        debug!("deployed {count} artifacts under {}", base.display());

        Ok(())
    }

    /// Parallel download a batch of files keyed by `K` and store them in the artifact storage.
    /// Each file is described by a 4-tuple of `(name, src, len, checksum)`,
    /// as specified in [`Self::download`].
    pub async fn batch_download<K>(
        &self,
        download: HashMap<
            K,
            (
                String,
                String,
                Option<u64>,
                impl IntoIterator<Item = Checksum> + Send,
            ),
        >,
    ) -> anyhow::Result<HashMap<K, Artifact>>
    where
        K: Eq + Hash,
    {
        let map = stream::iter(download)
            .map(|(k, (name, src, len, checksum))| async move {
                self.download(name, src, len, checksum)
                    .await
                    .map(|a| (k, a))
            })
            .buffer_unordered(self.config.parallel_download)
            .try_collect::<HashMap<_, _>>()
            .await?;

        Ok(map)
    }
}
