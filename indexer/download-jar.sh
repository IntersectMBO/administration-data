#!/bin/bash

# Script to download YACI Store JAR file
# Usage: ./download-jar.sh [version]

set -e

VERSION=${1:-"2.0.0-beta5"}
JAR_FILE="yaci-store-all-${VERSION}.jar"

# Try different URL formats
DOWNLOAD_URLS=(
    "https://github.com/bloxbean/yaci-store/releases/download/v${VERSION}/${JAR_FILE}"
    "https://github.com/bloxbean/yaci-store/releases/download/${VERSION}/${JAR_FILE}"
    "https://github.com/bloxbean/yaci-store/releases/latest/download/${JAR_FILE}"
)

echo "Downloading YACI Store ${VERSION}..."
echo "URL: ${DOWNLOAD_URL}"

if [ -f "${JAR_FILE}" ]; then
    echo "JAR file already exists: ${JAR_FILE}"
    exit 0
fi

# Try each URL format
DOWNLOAD_SUCCESS=false
for DOWNLOAD_URL in "${DOWNLOAD_URLS[@]}"; do
    echo "Trying: ${DOWNLOAD_URL}"
    if command -v curl &> /dev/null; then
        if curl -L -f -o "${JAR_FILE}" "${DOWNLOAD_URL}" 2>/dev/null; then
            DOWNLOAD_SUCCESS=true
            break
        fi
    elif command -v wget &> /dev/null; then
        if wget -q -O "${JAR_FILE}" "${DOWNLOAD_URL}" 2>/dev/null; then
            DOWNLOAD_SUCCESS=true
            break
        fi
    fi
done

if [ "$DOWNLOAD_SUCCESS" = false ]; then
    echo "Error: Failed to download JAR file from any URL"
    echo ""
    echo "Please download manually from:"
    echo "https://github.com/bloxbean/yaci-store/releases"
    echo ""
    echo "Look for a file named: ${JAR_FILE}"
    echo "Or download the latest yaci-store-all-*.jar file"
    exit 1
fi

# Verify the file is actually a JAR (not an error page)
if [ ! -s "${JAR_FILE}" ] || [ $(stat -f%z "${JAR_FILE}" 2>/dev/null || stat -c%s "${JAR_FILE}" 2>/dev/null) -lt 1000 ]; then
    echo "Error: Downloaded file appears to be invalid (too small)"
    rm -f "${JAR_FILE}"
    echo "Please download manually from: https://github.com/bloxbean/yaci-store/releases"
    exit 1
fi

echo "Successfully downloaded: ${JAR_FILE}"
ls -lh "${JAR_FILE}"
