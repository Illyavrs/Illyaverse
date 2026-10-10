# syntax=docker/dockerfile:1
FROM node:22-bookworm-slim AS builder
WORKDIR /illyaverse
ENV NEXT_TELEMETRY_DISABLED=1
COPY package.json package-lock.json ./
COPY apps/serendipity/package.json ./apps/serendipity/package.json
COPY apps/docs/package.json ./apps/docs/package.json
COPY tools/kaleidoscope/ui/package.json ./tools/kaleidoscope/ui/package.json
RUN npm ci --include=dev
COPY apps/docs ./apps/docs
RUN npm run build --workspace @illyaverse/docs

FROM node:22-bookworm-slim AS runtime
WORKDIR /illyaverse
ENV NODE_ENV=production NEXT_TELEMETRY_DISABLED=1 HOSTNAME=0.0.0.0 PORT=3000
COPY --from=builder --chown=node:node /illyaverse/apps/docs/.next/standalone ./
COPY --from=builder --chown=node:node /illyaverse/apps/docs/.next/static ./apps/docs/.next/static
USER node
EXPOSE 3000
CMD ["node", "apps/docs/server.js"]
