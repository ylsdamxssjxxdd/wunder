use super::SqliteStorage;
use crate::storage::{StorageLifecycle, VectorDocumentRecord, VectorDocumentSummaryRecord};
use anyhow::Result;
use rusqlite::{params, OptionalExtension};

pub(super) trait SqliteVectorDocumentStorage {
    fn upsert_vector_document_impl(&self, record: &VectorDocumentRecord) -> Result<()>;
    fn get_vector_document_impl(
        &self,
        owner_id: &str,
        base_name: &str,
        doc_id: &str,
    ) -> Result<Option<VectorDocumentRecord>>;
    fn list_vector_document_summaries_impl(
        &self,
        owner_id: &str,
        base_name: &str,
    ) -> Result<Vec<VectorDocumentSummaryRecord>>;
    fn delete_vector_document_impl(
        &self,
        owner_id: &str,
        base_name: &str,
        doc_id: &str,
    ) -> Result<bool>;
    fn delete_vector_documents_by_base_impl(&self, owner_id: &str, base_name: &str) -> Result<i64>;
}

impl SqliteVectorDocumentStorage for SqliteStorage {
    fn upsert_vector_document_impl(&self, record: &VectorDocumentRecord) -> Result<()> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        conn.execute(
            "INSERT INTO vector_documents \
             (doc_id, owner_id, base_name, doc_name, embedding_model, chunk_size, chunk_overlap, chunk_count, status, created_at, updated_at, content, chunks_json) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(doc_id) DO UPDATE SET \
             owner_id = excluded.owner_id, \
             base_name = excluded.base_name, \
             doc_name = excluded.doc_name, \
             embedding_model = excluded.embedding_model, \
             chunk_size = excluded.chunk_size, \
             chunk_overlap = excluded.chunk_overlap, \
             chunk_count = excluded.chunk_count, \
             status = excluded.status, \
             created_at = excluded.created_at, \
             updated_at = excluded.updated_at, \
             content = excluded.content, \
             chunks_json = excluded.chunks_json",
            params![
                record.doc_id,
                record.owner_id,
                record.base_name,
                record.doc_name,
                record.embedding_model,
                record.chunk_size,
                record.chunk_overlap,
                record.chunk_count,
                record.status,
                record.created_at,
                record.updated_at,
                record.content,
                record.chunks_json
            ],
        )?;
        Ok(())
    }

    fn get_vector_document_impl(
        &self,
        owner_id: &str,
        base_name: &str,
        doc_id: &str,
    ) -> Result<Option<VectorDocumentRecord>> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let row = conn
            .query_row(
                "SELECT doc_id, owner_id, base_name, doc_name, embedding_model, chunk_size, chunk_overlap, chunk_count, status, created_at, updated_at, content, chunks_json \
                 FROM vector_documents WHERE doc_id = ? AND owner_id = ? AND base_name = ?",
                params![doc_id, owner_id, base_name],
                |row| {
                    Ok(VectorDocumentRecord {
                        doc_id: row.get(0)?,
                        owner_id: row.get(1)?,
                        base_name: row.get(2)?,
                        doc_name: row.get(3)?,
                        embedding_model: row.get(4)?,
                        chunk_size: row.get::<_, i64>(5)?,
                        chunk_overlap: row.get::<_, i64>(6)?,
                        chunk_count: row.get::<_, i64>(7)?,
                        status: row.get(8)?,
                        created_at: row.get(9)?,
                        updated_at: row.get(10)?,
                        content: row.get(11)?,
                        chunks_json: row.get(12)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    fn list_vector_document_summaries_impl(
        &self,
        owner_id: &str,
        base_name: &str,
    ) -> Result<Vec<VectorDocumentSummaryRecord>> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let mut stmt = conn.prepare(
            "SELECT doc_id, doc_name, status, chunk_count, embedding_model, updated_at \
             FROM vector_documents WHERE owner_id = ? AND base_name = ? \
             ORDER BY updated_at DESC",
        )?;
        let rows = stmt.query_map(params![owner_id, base_name], |row| {
            Ok(VectorDocumentSummaryRecord {
                doc_id: row.get(0)?,
                doc_name: row.get(1)?,
                status: row.get(2)?,
                chunk_count: row.get::<_, i64>(3)?,
                embedding_model: row.get(4)?,
                updated_at: row.get(5)?,
            })
        })?;
        let mut output = Vec::new();
        for item in rows.flatten() {
            output.push(item);
        }
        Ok(output)
    }

    fn delete_vector_document_impl(
        &self,
        owner_id: &str,
        base_name: &str,
        doc_id: &str,
    ) -> Result<bool> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let affected = conn.execute(
            "DELETE FROM vector_documents WHERE doc_id = ? AND owner_id = ? AND base_name = ?",
            params![doc_id, owner_id, base_name],
        )?;
        Ok(affected > 0)
    }

    fn delete_vector_documents_by_base_impl(&self, owner_id: &str, base_name: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let affected = conn.execute(
            "DELETE FROM vector_documents WHERE owner_id = ? AND base_name = ?",
            params![owner_id, base_name],
        )?;
        Ok(affected as i64)
    }
}

#[cfg(test)]
mod tests {
    use super::SqliteStorage;
    use crate::storage::*;
    use tempfile::tempdir;

    fn build_storage() -> (SqliteStorage, tempfile::TempDir) {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("vector-document-store.db");
        let storage = SqliteStorage::new(db_path.to_string_lossy().to_string());
        storage.ensure_initialized().expect("initialize sqlite");
        (storage, dir)
    }

    fn document(doc_id: &str, doc_name: &str, updated_at: f64) -> VectorDocumentRecord {
        VectorDocumentRecord {
            doc_id: doc_id.to_string(),
            owner_id: "owner-a".to_string(),
            base_name: "base-a".to_string(),
            doc_name: doc_name.to_string(),
            embedding_model: "model-a".to_string(),
            chunk_size: 128,
            chunk_overlap: 16,
            chunk_count: 2,
            status: "ready".to_string(),
            created_at: 1.0,
            updated_at,
            content: "content".to_string(),
            chunks_json: "[]".to_string(),
        }
    }

    #[test]
    fn vector_document_roundtrip_orders_and_deletes() {
        let (storage, _dir) = build_storage();

        storage
            .upsert_vector_document(&document("doc-a", "Doc A", 1.0))
            .expect("upsert first document");
        storage
            .upsert_vector_document(&document("doc-b", "Doc B", 2.0))
            .expect("upsert second document");

        let mut updated = document("doc-a", "Doc A Updated", 3.0);
        updated.chunk_count = 4;
        storage
            .upsert_vector_document(&updated)
            .expect("update first document");

        assert_eq!(
            storage
                .get_vector_document("owner-a", "base-a", "doc-a")
                .expect("get document")
                .map(|record| (record.doc_name, record.chunk_count)),
            Some(("Doc A Updated".to_string(), 4))
        );
        assert_eq!(
            storage
                .list_vector_document_summaries("owner-a", "base-a")
                .expect("list summaries")
                .iter()
                .map(|record| record.doc_id.as_str())
                .collect::<Vec<_>>(),
            vec!["doc-a", "doc-b"]
        );
        assert!(storage
            .delete_vector_document("owner-a", "base-a", "doc-b")
            .expect("delete one document"));
        assert_eq!(
            storage
                .delete_vector_documents_by_base("owner-a", "base-a")
                .expect("delete base documents"),
            1
        );
        assert!(storage
            .list_vector_document_summaries("owner-a", "base-a")
            .expect("list after delete")
            .is_empty());
    }
}
