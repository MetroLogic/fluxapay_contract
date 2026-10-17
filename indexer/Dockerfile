FROM node:20-alpine

WORKDIR /app

# Copy package files
COPY package.json package-lock.json* ./

# Install dependencies
RUN npm ci

# Copy source code
COPY . .

# Build TypeScript
RUN npm run build

# Create non-root user and give it ownership of application files
RUN addgroup --system fluxapay && adduser --system --ingroup fluxapay fluxapay \
    && chown -R fluxapay:fluxapay /app

# Expose HTTP REST API port
EXPOSE 3001

# Run as non-root user
USER fluxapay

# Run migrations and start the indexer
CMD npm run migrate && npm start
