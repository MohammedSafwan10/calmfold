# 06 Sync server (Coolify on Oracle VPS)

Start only after notes, tasks and vault work well locally.

## Design

- `server/` crate: axum + sqlx + Postgres, Docker image, deployed by Coolify.
- Clients push/pull encrypted change records `{id, entity, updated_at, deleted_at, blob}`.
  Server never sees plaintext or keys.
- Conflict handling: last-writer-wins per record first; move note bodies to a
  CRDT (`loro`) when real-time multi-device editing matters.
- Auth: device registration + token; account key derived separately from vault key.

## Steps

1. [ ] `sync_state` table + change log in the client (migration)
2. [ ] Protocol types in a shared crate, versioned
3. [ ] Server: push/pull endpoints, health check, Dockerfile
4. [ ] Coolify deploy, TLS, backups of Postgres
5. [ ] Client sync loop (background, offline-tolerant), status in sidebar footer
6. [ ] Tests: two clients converge, offline edits, deletes, clock skew
