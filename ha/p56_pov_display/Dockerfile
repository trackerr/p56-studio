ARG BUILD_FROM
FROM ${BUILD_FROM}

ARG BUILD_ARCH
ARG P56_VERSION
ARG P56_REPO=trackerr/p56-studio

# p56-server is a static binary (Rust, musl) from the p56-studio release, checked against SHA256SUMS.
RUN set -eux; \
    case "${BUILD_ARCH}" in \
      amd64) target="x86_64" ;; \
      aarch64) target="aarch64" ;; \
      *) echo "unsupported architecture ${BUILD_ARCH}"; exit 1 ;; \
    esac; \
    file="p56-server-${target}-linux-musl"; \
    base="https://github.com/${P56_REPO}/releases/download/v${P56_VERSION}"; \
    cd /tmp; \
    curl -fsSL --retry 3 -o "${file}" "${base}/${file}"; \
    curl -fsSL --retry 3 -o SHA256SUMS "${base}/SHA256SUMS"; \
    grep " ${file}\$" SHA256SUMS | sha256sum -c -; \
    install -m 0755 "${file}" /usr/bin/p56-server; \
    rm -f "${file}" SHA256SUMS; \
    p56-server --version

COPY rootfs /

LABEL \
    io.hass.name="P56 POV Display" \
    io.hass.description="Holoscope/P56 POV fans in Home Assistant" \
    io.hass.arch="${BUILD_ARCH}" \
    io.hass.type="addon" \
    io.hass.version="${BUILD_VERSION}" \
    org.opencontainers.image.source="https://github.com/trackerr/home-assistant-p56"
