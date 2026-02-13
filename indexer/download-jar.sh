#!/bin/bash

# Script to download YACI Store JAR file
# Usage: ./download-jar.sh [version]

set -e

VERSION=${1:-"2.0.0-beta5"}
ZIP_FILE="yaci-store-${VERSION}.zip"
JAR_FILE="yaci-store.jar"

echo "Downloading YACI Store ${VERSION}..."

if [ -f "${JAR_FILE}" ]; then
    echo "JAR file already exists: ${JAR_FILE}"
    exit 0
fi

DOWNLOAD_URL="https://github.com/bloxbean/yaci-store/releases/download/v${VERSION}/${ZIP_FILE}"

echo "Trying: ${DOWNLOAD_URL}"
if command -v curl &> /dev/null; then
    curl -L -f -o "${ZIP_FILE}" "${DOWNLOAD_URL}"
elif command -v wget &> /dev/null; then
    wget -q -O "${ZIP_FILE}" "${DOWNLOAD_URL}"
else
    echo "Error: Neither curl nor wget found"
    exit 1
fi

if [ ! -s "${ZIP_FILE}" ]; then
    echo "Error: Failed to download ${ZIP_FILE}"
    echo "Please download manually from: https://github.com/bloxbean/yaci-store/releases"
    rm -f "${ZIP_FILE}"
    exit 1
fi

# Extract the JAR from the zip
echo "Extracting ${JAR_FILE} from ${ZIP_FILE}..."
unzip -j -o "${ZIP_FILE}" "yaci-store-${VERSION}/${JAR_FILE}" -d .

rm -f "${ZIP_FILE}"

if [ ! -s "${JAR_FILE}" ]; then
    echo "Error: Extracted JAR file is missing or empty"
    exit 1
fi

echo "Successfully downloaded: ${JAR_FILE}"
ls -lh "${JAR_FILE}"
