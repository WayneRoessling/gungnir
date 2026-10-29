# CI: the from-source image and the libproj job when a change can reach them

The second batch from this date's CI review, after
`ci-mold-advisories-off-the-pull-request-path.md`. Two jobs ran on every push or pull
request whether or not the change could reach what they check.

## What the runs showed

- **The container image from source, on every push to main.** `node-linux-container`
  builds the node inside the Dockerfile's toolchain stage with a warm cache in about two
  minutes, puts the binary into the image and starts it; that is the check. Then, on
  main only, it built the image again from source, cold, through the Dockerfile's own
  `COPY . .` stage: 9.8 to 10.3 minutes on each of 48 pushes in 30 days, about eight
  hours of runner time a month, for an image nothing consumed, since release.yml builds
  its own on a tag. On a pull request the same step already ran only when the change
  touched the manifests, the lockfile, the toolchain file, `deploy/` or `.dockerignore`.
- **The libproj job, on every code pull request.** 3.7 to 4.5 minutes in parallel and
  2.2 GB of the 10 GB cache. Its feature-gated code is in three crates (`gungnir-data`,
  `gungnir-app`, `gungnir-node`), its fixtures under four `testdata/` directories, its
  grids in `deploy/geoid/`, and its build in the lockfile and the workspace manifest.
  Most pull requests touch none of those.

## What changed

- **A push to main scopes the from-source build the way a pull request does.** A push
  to main is one squash commit, or a merge commit whose first parent is the previous
  main, so the two-commit diff the job already used on a pull request applies to a
  push. The from-source build runs on main when the merge touched those files.
- **`changes` has a second output, `crs`**, true when the diff touches the three crates,
  the four fixture directories, `deploy/geoid/`, the lockfile, the workspace manifest or
  `ci.yml`. `proj-crs` runs on a pull request only then. Every push to main still runs
  it, so a change that slipped past the list is caught on main and the caches stay warm.
- **A weekly scheduled run on main, and `workflow_dispatch`.** The weekly run builds
  everything, the image from source and the libproj job included, against whatever the
  week merged. It also restores every cache: GitHub drops a cache not restored for seven
  days, which is how a month without a push (#200, 2026-10-23) left every pull request
  building cold at its 30-minute limit.

## What is not scoped, and why

The ordinary `test` job compiles `gungnir-app` without the `crs` feature on every code
pull request, so a change anywhere under it that breaks the crate is still caught
there. What a pull request outside the listed paths can no longer break unseen is the
feature-gated surface itself, and that surface lives in the listed paths.

## What the pull request's run showed

This pull request changes `ci.yml`, which is in the `crs` list, so its own run ran the
libproj job; it touches no manifest or `deploy/` file, so the from-source image build
was skipped on the pull request and will be on main's push for the merge.
