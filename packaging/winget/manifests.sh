#!/bin/sh
# Writes the winget manifests for microsoft/winget-pkgs
# (manifests/t/tanganyika/wireant/<version>/):
#   packaging/winget/manifests.sh <version> <sha256 of the setup .exe> <release date YYYY-MM-DD> <out dir>
# Users: winget install tanganyika.wireant
set -eu
VER=$1
SHA=$(printf '%s' "$2" | tr a-f A-F)
DATE=$3
OUT=$4
ID=tanganyika.wireant
MV=1.9.0
URL=https://github.com/tanganyika/wireant/releases/download/v$VER/wireant-$VER-windows-x64-setup.exe
mkdir -p "$OUT"

cat > "$OUT/$ID.yaml" <<YAML
# yaml-language-server: \$schema=https://aka.ms/winget-manifest.version.$MV.schema.json
PackageIdentifier: $ID
PackageVersion: $VER
DefaultLocale: en-US
ManifestType: version
ManifestVersion: $MV
YAML

cat > "$OUT/$ID.installer.yaml" <<YAML
# yaml-language-server: \$schema=https://aka.ms/winget-manifest.installer.$MV.schema.json
PackageIdentifier: $ID
PackageVersion: $VER
InstallerType: inno
UpgradeBehavior: install
ReleaseDate: $DATE
MinimumOSVersion: 10.0.0.0
FileExtensions:
- ez
- maa
- nec
ProductCode: '{E89BB01A-6011-4002-82AC-5B89A6F40F89}_is1'
Installers:
- Architecture: x64
  Scope: user
  InstallerUrl: $URL
  InstallerSha256: $SHA
  InstallerSwitches:
    Custom: /CURRENTUSER
- Architecture: x64
  Scope: machine
  InstallerUrl: $URL
  InstallerSha256: $SHA
  ElevationRequirement: elevatesSelf
  InstallerSwitches:
    Custom: /ALLUSERS
ManifestType: installer
ManifestVersion: $MV
YAML

cat > "$OUT/$ID.locale.en-US.yaml" <<YAML
# yaml-language-server: \$schema=https://aka.ms/winget-manifest.defaultLocale.$MV.schema.json
PackageIdentifier: $ID
PackageVersion: $VER
PackageLocale: en-US
Publisher: tanganyika
PublisherUrl: https://github.com/tanganyika
PublisherSupportUrl: https://github.com/tanganyika/wireant/issues
PackageName: wireant
PackageUrl: https://github.com/tanganyika/wireant
License: GPL-3.0-or-later
LicenseUrl: https://github.com/tanganyika/wireant/blob/main/LICENSE
ShortDescription: Wire antenna modelling with MININEC, wiremom and NEC-2 engines
Description: |-
  wireant models wire antennas by the method of moments: impedance, SWR, gain and patterns,
  frequency sweeps, an optimizer, matching networks, stacked arrays, a great circle map and
  RF exposure checks. It reads MMANA-GAL (.maa), NEC (.nec) and EZNEC (.ez) files.
Moniker: wireant
Tags:
- amateur-radio
- antenna
- ham-radio
- mininec
- nec
- simulation
ReleaseNotesUrl: https://github.com/tanganyika/wireant/releases/tag/v$VER
ManifestType: defaultLocale
ManifestVersion: $MV
YAML
echo "$OUT"
