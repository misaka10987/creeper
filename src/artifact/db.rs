use anyhow::bail;
use sqlx::{AssertSqlSafe, query, query_as};
use tracing::warn;

use crate::{Artifact, artifact::ArtifactManager, checksum::HashFunc};

impl ArtifactManager {
    pub(super) async fn select(
        &self,
        hash: HashFunc,
        checksum: &str,
    ) -> anyhow::Result<Option<Artifact>> {
        // `.bind()` can not bind column names
        let query = format!("SELECT * FROM artifact WHERE {hash} = ?");

        // this is safe because `HashFunc` is a finite enum and has a known string representation
        let query = AssertSqlSafe(query);

        let found = query_as(query)
            .bind(checksum)
            .fetch_optional(&self.index)
            .await?;
        Ok(found)
    }

    pub(super) async fn insert(&self, artifact: &Artifact) -> anyhow::Result<()> {
        if self.get(&artifact.blake3).await?.is_some() {
            warn!("duplicate add of artifact, this is likely due to an inefficient design");
            return Ok(());
        }

        query("INSERT INTO artifact (blake3, name, src, len, sha1, sha256, md5) VALUES (?, ?, ?, ?, ?, ?, ?)")
        .bind(&artifact.blake3)
        .bind(&artifact.name)
        .bind(&artifact.src)
        .bind(artifact.len as i64)
        .bind(&artifact.sha1)
        .bind(&artifact.sha256)
        .bind(&artifact.md5)
        .execute(&self.index)
        .await?;
        Ok(())
    }

    pub(super) async fn update(&self, art: &Artifact) -> anyhow::Result<()> {
        let r = query("UPDATE artifact SET sha1 = ?, sha256 = ?, md5 = ? WHERE blake3 = ?")
            .bind(&art.sha1)
            .bind(&art.sha256)
            .bind(&art.md5)
            .bind(&art.blake3)
            .execute(&self.index)
            .await?;

        match r.rows_affected() {
            0 => bail!("no artifact to update"),
            1 => Ok(()),
            _ => panic!("duplicate blake3 (primary key)"),
        }
    }
}
