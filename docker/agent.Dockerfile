# charet-agent: starts a container per web agent run (through the Docker socket) and is those
# containers' only way out: a relay to the server's agent door and an HTTPS proxy to model APIs.

FROM node:24-trixie-slim
COPY apps/charet-agent/package.json /app/package.json
COPY apps/charet-agent/src /app/src
# Root: it drives Docker through the socket, which is root-equivalent anyway.
ENV CHARET_AGENT_BIND=0.0.0.0:5290 \
    CHARET_AGENT_DOOR_BIND=0.0.0.0:5291 \
    CHARET_AGENT_PROXY_BIND=0.0.0.0:5292
EXPOSE 5290 5291 5292
CMD ["node", "/app/src/main.ts"]
