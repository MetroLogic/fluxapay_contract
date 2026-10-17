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

# Expose HTTP REST API port
EXPOSE 3001

# Run migrations and start the indexer
CMD npm run migrate && npm start
