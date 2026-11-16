#!/bin/sh
# Adds a .deb to the APT repository kept on the gh-pages branch (GitHub Pages) and signs it.
#
#   packaging/linux/apt-repo.sh <site dir> <wireant_X.Y.Z_amd64.deb> [versions to keep, default 3]
#
# <site dir> is a checkout of gh-pages; the repository goes in <site dir>/apt:
#   apt/pool/main/wireant_*.deb, apt/dists/stable/{InRelease,Release,Release.gpg},
#   apt/dists/stable/main/binary-amd64/Packages(.gz), apt/wireant.gpg (the public key).
# It signs with the default key in gpg's keyring (the release workflow imports the
# APT_GPG_PRIVATE_KEY secret first). One amd64 package serves Ubuntu 20.04+, Debian 11+,
# Linux Mint and the rest, as it needs only glibc 2.31.
set -eu

SITE=$1
DEB=$2
KEEP=${3:-3}
URL=${APT_URL:-https://tanganyika.github.io/wireant/apt}

R="$SITE/apt"
mkdir -p "$R/pool/main" "$R/dists/stable/main/binary-amd64"
cp "$DEB" "$R/pool/main/"
# keep only the newest few versions
ls "$R/pool/main"/wireant_*_amd64.deb | sort -V | head -n -"$KEEP" | xargs -r rm -f

cd "$R"
apt-ftparchive packages pool > dists/stable/main/binary-amd64/Packages
gzip -9 -n -k -f dists/stable/main/binary-amd64/Packages
# (the old Release files mustn't be listed in the new one)
rm -f dists/stable/Release dists/stable/InRelease dists/stable/Release.gpg
apt-ftparchive \
  -o APT::FTPArchive::Release::Origin=wireant \
  -o APT::FTPArchive::Release::Label=wireant \
  -o APT::FTPArchive::Release::Suite=stable \
  -o APT::FTPArchive::Release::Codename=stable \
  -o APT::FTPArchive::Release::Architectures=amd64 \
  -o APT::FTPArchive::Release::Components=main \
  -o APT::FTPArchive::Release::Description="wireant wire antenna modelling" \
  release dists/stable > Release.tmp
mv Release.tmp dists/stable/Release
gpg --batch --yes --clearsign -o dists/stable/InRelease dists/stable/Release
gpg --batch --yes --armor --detach-sign -o dists/stable/Release.gpg dists/stable/Release
gpg --batch --yes --export -o wireant.gpg

cat > index.html <<EOF
<!doctype html>
<html lang="en"><head><meta charset="utf-8"><title>wireant APT repository</title>
<meta name="viewport" content="width=device-width, initial-scale=1"></head>
<body style="font-family:sans-serif;max-width:46em;margin:2em auto;padding:0 1em">
<h1>wireant APT repository</h1>
<p>For Ubuntu 20.04 and later, Debian 11 and later, Linux Mint, Zorin, Pop!_OS (64-bit PC).
Add it once, and wireant updates with the rest of the system:</p>
<pre>sudo install -d -m 0755 /etc/apt/keyrings
curl -fsSL $URL/wireant.gpg | sudo tee /etc/apt/keyrings/wireant.gpg &gt;/dev/null
echo "deb [arch=amd64 signed-by=/etc/apt/keyrings/wireant.gpg] $URL stable main" | sudo tee /etc/apt/sources.list.d/wireant.list
sudo apt update
sudo apt install wireant</pre>
<p>Packages: $(cd pool/main && ls -1 | sed 's/$/ /' | tr -d '\n')</p>
<p><a href="https://github.com/tanganyika/wireant">wireant on GitHub</a></p>
</body></html>
EOF
echo "APT repository updated in $R"
