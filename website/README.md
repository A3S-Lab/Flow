# A3S Flow documentation website

This directory contains the bilingual, versioned A3S Flow documentation built with Rspress. Chinese is the default locale. The current release is mounted at `/Flow/`, English at `/Flow/en/`, and archived releases under `/Flow/<version>/`.

## Local development

Install the exact dependency set and start the development server from the repository root:

```sh
npm ci --prefix website
npm run dev --prefix website
```

The production site uses the `/Flow/` base path. Build and verify the same shape locally with:

```sh
DOCS_BASE=/Flow/ DOCS_ORIGIN=https://a3s-lab.github.io npm run build --prefix website
DOCS_BASE=/Flow/ npm run check:site --prefix website
```

Run all source checks before committing documentation changes:

```sh
npm run check --prefix website
```

The custom-node catalog has a deterministic A3S Test suite. Start the local
site on the suite's fixed origin, then validate and run it from `website/`:

```sh
npm run dev -- --host 127.0.0.1 --port 4173
a3s-test check tests/e2e/workflow-custom-nodes.acl --json
a3s-test run tests/e2e/workflow-custom-nodes.acl --browser-driver standalone --browser-executable agent-browser --json
```

## Current homepage

The current release uses a product homepage centered on the AI Native Workflow Engine. It explains the shared node manifests, React and Vue components, CLI, Skill, graph compiler, and durable runtime as one system. The homepage uses real `@a3s-lab/flow-ui` node components rather than a separate playground implementation.

Homepage copy lives in `theme/components/HomeCopy.ts`. Structure and product scenes live in `theme/components/HomeLayout.tsx` and `theme/components/HomeVisuals.tsx`. Keep Chinese and English copy synchronized, preserve the visible locale controls, and add stable output assertions to `scripts/check-built-site.mjs` when the narrative changes.

## Version and locale layout

Every published version has two route-identical trees:

```text
docs/
├── v1.0.0/
│   ├── zh/
│   └── en/
├── v0.13.1/
│   ├── zh/
│   └── en/
└── v0.12.0/
    ├── zh/
    └── en/
```

The relative MDX path is the route contract. If `zh/operations/persistence.mdx` exists, the matching `en/operations/persistence.mdx` must exist in the same version. Keep `_meta.json` and `_nav.json` synchronized with each locale's page tree. Chinese and English wording can differ, but code examples and version-specific behavior must describe the same release.

`versions.mjs` declares the visible version order and the default version. Rspress omits the default version from its public URL. Archived versions retain the version prefix. Pages that exist only in the current release fall back to the selected archive's overview when a reader changes versions.

## Adding or updating a release snapshot

1. Confirm behavior against the release tag rather than the current branch.
2. Add matching `zh` and `en` page trees under `docs/<version>/`.
3. Update the ordered version list and default version in `versions.mjs`.
4. Record the exact 40-character source commit and release timestamp in `version-snapshots.json`.
5. Update the expected page count in `scripts/check-content.mjs`.
6. Update the current-only route set in `theme/components/Nav.tsx` when the new current release adds routes that archives do not contain.
7. Run the source, build, and output checks shown above.

Do not silently add current APIs to an archived snapshot. If an older page needs a correction, keep its examples pinned to that release and describe only behavior available at its recorded source commit.

## GitHub Pages verification

`.github/workflows/pages.yml` runs on pull requests and on pushes to `main`. Pull requests build and validate the site. A push to `main` also uploads `website/doc_build` and deploys it to GitHub Pages.

After deployment, verify these routes in a browser:

- `/Flow/` opens the Chinese current homepage.
- `/Flow/en/` opens the English current homepage.
- Both language controls preserve the selected version.
- All version controls reach real snapshots, including `v0.13.1` and `v0.12.0`.
- Desktop and mobile navigation expose search, language, version, and repository links.
- No asset request escapes the `/Flow/` base path.
