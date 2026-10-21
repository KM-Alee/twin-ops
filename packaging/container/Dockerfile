# twin as an MCP server in a container. The host is mounted read-only at /host; twin
# reads /host/proc, unit files, package databases, configuration and the Docker,
# containerd and D-Bus sockets under it and never writes there.
#
# The release workflow copies the static musl binaries to dist/<arch>/twin first.
FROM alpine:3.20 AS certs
RUN apk add --no-cache ca-certificates

FROM scratch
ARG TARGETARCH
COPY --from=certs /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
COPY dist/${TARGETARCH}/twin /usr/bin/twin
ENV TWIN_HOST_ROOT=/host \
    HOME=/data \
    XDG_DATA_HOME=/data \
    XDG_CONFIG_HOME=/data \
    XDG_STATE_HOME=/data
VOLUME /data
LABEL io.modelcontextprotocol.server.name="io.github.KM-Alee/twin" \
      org.opencontainers.image.source="https://github.com/KM-Alee/twin-ops" \
      org.opencontainers.image.description="Read-only dependency map and blast-radius check for Linux hosts, for AI agents" \
      org.opencontainers.image.licenses="MIT"
ENTRYPOINT ["/usr/bin/twin"]
CMD ["mcp"]
