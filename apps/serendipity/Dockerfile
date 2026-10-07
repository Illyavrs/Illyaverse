# syntax=docker/dockerfile:1
FROM node:22-bookworm-slim AS builder
WORKDIR /illyaverse
ENV NEXT_TELEMETRY_DISABLED=1
COPY package.json package-lock.json ./
COPY apps/serendipity/package.json ./apps/serendipity/package.json
RUN npm ci --include=dev
COPY apps/serendipity ./apps/serendipity
COPY docs ./docs
ARG API_INTERNAL_URL=http://sanctuary:8080
ENV API_INTERNAL_URL=${API_INTERNAL_URL}
RUN npm run build

FROM node:22-bookworm-slim AS runtime
WORKDIR /illyaverse
ENV NODE_ENV=production NEXT_TELEMETRY_DISABLED=1 HOSTNAME=0.0.0.0 PORT=3000 ILLYA_DOCS_PATH=/illyaverse/docs API_INTERNAL_URL=http://sanctuary:8080
COPY --from=builder --chown=node:node /illyaverse/apps/serendipity/.next/standalone ./
COPY --from=builder --chown=node:node /illyaverse/apps/serendipity/public ./apps/serendipity/public
COPY --from=builder --chown=node:node /illyaverse/apps/serendipity/.next/static ./apps/serendipity/.next/static
COPY --from=builder --chown=node:node /illyaverse/docs ./docs
USER node
EXPOSE 3000
CMD ["node", "apps/serendipity/server.js"]
