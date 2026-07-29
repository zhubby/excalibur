FROM node:22-bookworm-slim AS deps
WORKDIR /app
COPY frontend/package.json frontend/bun.lockb* ./
RUN npm install -g bun@1.0.4 && bun install --frozen-lockfile

FROM node:22-bookworm-slim AS builder
WORKDIR /app
COPY --from=deps /app/node_modules ./node_modules
COPY frontend ./
RUN npm run build

FROM node:22-bookworm-slim AS runner
WORKDIR /app
ENV NODE_ENV=production
COPY --from=builder /app/.next/standalone ./
COPY --from=builder /app/.next/static ./.next/static
COPY --from=builder /app/public ./public
EXPOSE 3000
CMD ["node", "server.js"]
