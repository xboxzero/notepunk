# NotePunk — Supabase setup (for the public board)

The board tab needs a Supabase project. Without it, notes still work locally; only the board is dark.

## 1. Make a project

1. Go to https://supabase.com and create a free project.
2. Wait for provisioning. Grab two values from **Project Settings → API**:
   - Project URL (e.g. `https://abcdxyz.supabase.co`)
   - `anon` public API key

## 2. Run the schema

Open **SQL Editor** in Supabase and run:

```sql
create extension if not exists "pgcrypto";

create table if not exists posts (
    id uuid primary key default gen_random_uuid(),
    title text not null default '',
    body text not null default '',
    tags text[] not null default '{}',
    author text not null default 'anon',
    created_at timestamptz not null default now(),
    updated_at timestamptz not null default now()
);

create table if not exists comments (
    id uuid primary key default gen_random_uuid(),
    post_id uuid not null references posts(id) on delete cascade,
    body text not null,
    author text not null default 'anon',
    created_at timestamptz not null default now()
);

create index if not exists posts_created_idx on posts(created_at desc);
create index if not exists comments_post_idx on comments(post_id, created_at);

alter table posts enable row level security;
alter table comments enable row level security;

create policy "posts_read_all"   on posts    for select using (true);
create policy "posts_insert_anon" on posts   for insert with check (true);
create policy "comments_read_all" on comments for select using (true);
create policy "comments_insert_anon" on comments for insert with check (true);
```

This makes the board append-only and world-readable. Rate-limiting and abuse mitigation are not configured — add them later via Supabase Edge Functions or move to authenticated insert if you start getting spam.

## 3. Wire up the keys

Edit `index.html`:

```html
<script>
  window.NOTEPUNK_CONFIG = {
    supabase_url: "https://abcdxyz.supabase.co",
    supabase_anon_key: "eyJhbGci...your-anon-key..."
  };
</script>
```

Commit and push. The anon key is safe to publish — it's designed for browser exposure and is gated by the RLS policies above.

## 4. Use it

- Set your handle in the **board** tab (top bar). Saved to localStorage.
- Hit **publish to board** on any note in the **notes** tab. Title/body/tags are sent to Supabase.
- Click any post to read + comment.
