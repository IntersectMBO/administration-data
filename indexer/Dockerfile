FROM eclipse-temurin:21-jre-alpine

WORKDIR /app

# Install netcat for health checks
RUN apk add --no-cache netcat-openbsd

# Copy YACI Store JAR
COPY yaci-store.jar yaci-store.jar

# Copy configuration files
COPY application.properties /app/application.properties
COPY docker-entrypoint.sh /app/docker-entrypoint.sh

# Make entrypoint executable
RUN chmod +x /app/docker-entrypoint.sh

# Expose YACI Store API port
EXPOSE 8080

ENTRYPOINT ["/app/docker-entrypoint.sh"]
