#!/bin/sh
# Opens a pull request on microsoft/winget-pkgs with the manifests for a release, from the
# gh user's fork (made if missing), without cloning the huge repository:
#   packaging/winget/submit.sh <version> <sha256 of the setup .exe> <release date YYYY-MM-DD>
# Needs gh logged in (or GH_TOKEN) with the public_repo scope.
set -eu
VER=$1
SHA=$2
DATE=$3
ID=tanganyika.wireant
DIR=manifests/t/tanganyika/wireant/$VER
ME=$(gh api user -q .login)
FORK=$ME/winget-pkgs
HERE=$(dirname "$0")
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

"$HERE/manifests.sh" "$VER" "$SHA" "$DATE" "$TMP" >/dev/null
if gh api "repos/microsoft/winget-pkgs/contents/manifests/t/tanganyika/wireant" >/dev/null 2>&1; then
  TITLE="New version: $ID version $VER"
else
  TITLE="New package: $ID version $VER"
fi

gh api "repos/$FORK" >/dev/null 2>&1 || gh repo fork microsoft/winget-pkgs --clone=false >/dev/null 2>&1 || true
for _ in $(seq 30); do gh api "repos/$FORK" >/dev/null 2>&1 && break; sleep 10; done
gh repo sync "$FORK" --branch master >/dev/null

BASE=$(gh api "repos/$FORK/git/ref/heads/master" -q .object.sha)
BASE_TREE=$(gh api "repos/$FORK/git/commits/$BASE" -q .tree.sha)
python3 -I - "$TMP" "$DIR" "$BASE_TREE" > "$TMP/tree.json" <<'PY'
import json, os, sys
tmp, d, base = sys.argv[1:]
files = sorted(f for f in os.listdir(tmp) if f.endswith(".yaml"))
print(json.dumps({"base_tree": base, "tree": [
    {"path": f"{d}/{f}", "mode": "100644", "type": "blob", "content": open(os.path.join(tmp, f)).read()} for f in files]}))
PY
TREE=$(gh api "repos/$FORK/git/trees" --input "$TMP/tree.json" -q .sha)
COMMIT=$(gh api "repos/$FORK/git/commits" -f message="$TITLE" -f tree="$TREE" -f "parents[]=$BASE" -q .sha)
BR=$ID-$VER
if gh api "repos/$FORK/git/ref/heads/$BR" >/dev/null 2>&1; then
  gh api -X PATCH "repos/$FORK/git/refs/heads/$BR" -f sha="$COMMIT" -F force=true >/dev/null
else
  gh api "repos/$FORK/git/refs" -f ref="refs/heads/$BR" -f sha="$COMMIT" >/dev/null
fi

gh pr create -R microsoft/winget-pkgs --base master --head "$ME:$BR" --title "$TITLE" --body "$(cat <<MD
$TITLE: wireant, a wire antenna modelling program (GPL-3.0). Inno Setup installer, per-user by default (/CURRENTUSER) or for all users (/ALLUSERS).

Release: https://github.com/tanganyika/wireant/releases/tag/v$VER

- [x] Have you checked that there aren't other open [pull requests](https://github.com/microsoft/winget-pkgs/pulls) for the same manifest update/change?
- [x] This PR only modifies one (1) manifest
- [ ] Have you [validated](https://github.com/microsoft/winget-pkgs/blob/master/doc/Authoring.md#validation) your manifest locally with \`winget validate --manifest <path>\`? (written by a script on Linux; checked against the schema layout only)
- [ ] Have you tested your manifest locally with \`winget install --manifest <path>\`?
MD
)"
