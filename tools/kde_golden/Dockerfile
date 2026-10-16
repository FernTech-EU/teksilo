# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech
#
# Reference ("oracle") environment for the KDE Breeze preset: KDE neon User
# Edition on Ubuntu 24.04, upgraded to Plasma 6.8 and Frameworks 6.31 from
# neon's user archive. See README.md for the versions it produced and why the
# upgrade step cannot be pinned more tightly than major.minor.
#
# Nothing here runs in CI. Build and run it through run.sh.

# invent-registry.kde.org/neon/docker-images/plasma:user, pulled 2026-10-16.
# That tag shipped Plasma 6.7.4 and Frameworks 6.29; the upgrade below brings
# it to 6.8 / 6.31.
FROM invent-registry.kde.org/neon/docker-images/plasma@sha256:f58f9b97f218ee976cf928f6fe00abb929a644aded8c9dfb02f77a9b2a6e1732

USER root
ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update \
 && apt-get full-upgrade -y \
 && apt-get install -y --no-install-recommends \
      cmake g++ make extra-cmake-modules \
      qt6-base-dev libkf6colorscheme-dev libkf6guiaddons-dev libkf6config-dev \
      kde-style-breeze kde-style-oxygen plasma-workspace \
      fonts-noto-core fonts-hack \
      python3 dbus \
 && rm -rf /var/lib/apt/lists/*

# Refuse an image that did not reach the versions the vectors are meant to
# describe. neon keeps only the newest build of each package, so a rebuild
# months from now can land on 6.8.N or 6.32: the check pins major.minor and
# versions.txt records exactly what was installed.
RUN set -e; \
    v() { dpkg-query -W -f='${Version}' "$1"; }; \
    case "$(v plasma-workspace)" in 4:6.8.*) ;; *) echo "plasma-workspace $(v plasma-workspace) is not 6.8.x" >&2; exit 1;; esac; \
    case "$(v kde-style-breeze)" in 4:6.8.*) ;; *) echo "kde-style-breeze $(v kde-style-breeze) is not 6.8.x" >&2; exit 1;; esac; \
    case "$(v kf6-kcolorscheme)" in 6.31.*) ;; *) echo "kf6-kcolorscheme $(v kf6-kcolorscheme) is not 6.31.x" >&2; exit 1;; esac; \
    case "$(v kf6-kguiaddons)" in 6.31.*) ;; *) echo "kf6-kguiaddons $(v kf6-kguiaddons) is not 6.31.x" >&2; exit 1;; esac; \
    mkdir -p /opt/kde_golden; \
    dpkg-query -W -f='${Package}\t${Version}\n' \
      plasma-workspace kde-style-breeze breeze kde-style-oxygen \
      kf6-kcolorscheme kf6-kguiaddons kf6-kconfig libkf6colorscheme-dev \
      qt6-base fonts-noto-core fonts-hack \
      | sort > /opt/kde_golden/versions.txt; \
    sha256sum /usr/share/fonts/truetype/noto/NotoSans-Regular.ttf \
              /usr/share/fonts/truetype/hack/Hack-Regular.ttf \
      >> /opt/kde_golden/fonts.sha256; \
    cat /opt/kde_golden/versions.txt

COPY driver/ /opt/kde_golden/driver/
RUN cmake -S /opt/kde_golden/driver -B /opt/kde_golden/build -DCMAKE_BUILD_TYPE=Release \
 && cmake --build /opt/kde_golden/build -j"$(nproc)"

USER neon
WORKDIR /home/neon
