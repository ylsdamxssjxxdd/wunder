use super::PostgresStorage;
use crate::storage::{StorageBackend, VectorDocumentRecord, VectorDocumentSummaryRecord};
use anyhow::Result;

pub(super) trait PostgresVectorDocumentStorage {
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

impl PostgresVectorDocumentStorage for PostgresStorage {
    fn upsert_vector_document_impl(&self, record: &VectorDocumentRecord) -> Result<()> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        conn.execute(
            "INSERT INTO vector_documents \
             (doc_id, owner_id, base_name, doc_name, embedding_model, chunk_size, chunk_overlap, chunk_count, status, created_at, updated_at, content, chunks_json) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13) \
             ON CONFLICT (doc_id) DO UPDATE SET \
             owner_id = EXCLUDED.owner_id, \
             base_name = EXCLUDED.base_name, \
             doc_name = EXCLUDED.doc_name, \
             embedding_model = EXCLUDED.embedding_model, \
             chunk_size = EXCLUDED.chunk_size, \
             chunk_overlap = EXCLUDED.chunk_overlap, \
             chunk_count = EXCLUDED.chunk_count, \
             status = EXCLUDED.status, \
             created_at = EXCLUDED.created_at, \
             updated_at = EXCLUDED.updated_at, \
             content = EXCLUDED.content, \
             chunks_json = EXCLUDED.chunks_json",
            &[
                &record.doc_id,
                &record.owner_id,
                &record.base_name,
                &record.doc_name,
                &record.embedding_model,
                &record.chunk_size,
                &record.chunk_overlap,
                &record.chunk_count,
                &record.status,
                &record.created_at,
                &record.updated_at,
                &record.content,
                &record.chunks_json,
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
        let mut conn = self.conn()?;
        let row = conn.query_opt(
            "SELECT doc_id, owner_id, base_name, doc_name, embedding_model, chunk_size, chunk_overlap, chunk_count, status, created_at, updated_at, content, chunks_json \
             FROM vector_documents WHERE doc_id = $1 AND owner_id = $2 AND base_name = $3",
            &[&doc_id, &owner_id, &base_name],
        )?;
        Ok(row.map(|row| VectorDocumentRecord {
            doc_id: row.get(0),
            owner_id: row.get(1),
            base_name: row.get(2),
            doc_name: row.get(3),
            embedding_model: row.get(4),
            chunk_size: row.get::<_, i64>(5),
            chunk_overlap: row.get::<_, i64>(6),
            chunk_count: row.get::<_, i64>(7),
            status: row.get(8),
            created_at: row.get(9),
            updated_at: row.get(10),
            content: row.get(11),
            chunks_json: row.get(12),
        }))
    }

    fn list_vector_document_summaries_impl(
        &self,
        owner_id: &str,
        base_name: &str,
    ) -> Result<Vec<VectorDocumentSummaryRecord>> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let rows = conn.query(
            "SELECT doc_id, doc_name, status, chunk_count, embedding_model, updated_at \
             FROM vector_documents WHERE owner_id = $1 AND base_name = $2 \
             ORDER BY updated_at DESC",
            &[&owner_id, &base_name],
        )?;
        let mut output = Vec::new();
        for row in rows {
            output.push(VectorDocumentSummaryRecord {
                doc_id: row.get(0),
                doc_name: row.get(1),
                status: row.get(2),
                chunk_count: row.get::<_, i64>(3),
                embedding_model: row.get(4),
                updated_at: row.get(5),
            });
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
        let mut conn = self.conn()?;
        let affected = conn.execute(
            "DELETE FROM vector_documents WHERE doc_id = $1 AND owner_id = $2 AND base_name = $3",
            &[&doc_id, &owner_id, &base_name],
        )?;
        Ok(affected > 0)
    }

    fn delete_vector_documents_by_base_impl(&self, owner_id: &str, base_name: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let affected = conn.execute(
            "DELETE FROM vector_documents WHERE owner_id = $1 AND base_name = $2",
            &[&owner_id, &base_name],
        )?;
        Ok(affected as i64)
    }
}
