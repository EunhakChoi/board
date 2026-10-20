# charet-workspace: installs and builds project commits. The only place project code runs.

FROM node:24-trixie-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates git \
    && rm -rf /var/lib/apt/lists/* \
    && npm install --global pnpm@12.5.1 \
    && mkdir -p /data && chown node /data
COPY apps/charet-workspace/package.json /app/package.json
COPY apps/charet-workspace/src /app/src
USER node
ENV CHARET_WORKSPACE_HOME=/data \
    CHARET_WORKSPACE_BIND=0.0.0.0:5282
EXPOSE 5282
VOLUME /data
CMD ["node", "/app/src/main.ts"]
