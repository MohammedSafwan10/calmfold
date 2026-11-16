# Calmfold website scaffold

This directory preserves the initial Astro configuration, brand assets,
sample-data app screenshots, design references and changelog. It is not yet a
finished or deployable website: there are no page routes.

Use Node 22.12 or newer and `npm ci`. Astro is pinned to 7.4.12; the dependency
audit reported zero known vulnerabilities on 2026-11-16.

`npm run build` first fetches the public Calmfold release feed. At this snapshot
the feed returns 404 and `src/data/release.json` has not been created, so that
step fails instead of publishing invented download links. Publish the real feed
or provide verified saved release metadata, then implement and review pages
before deployment. `public/assets` contains design references; copied examples
are not finished Calmfold product pages.
