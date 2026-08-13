#!/usr/bin/env node

/**
 * Capture a dated, source-preserving snapshot of CKB's VS Code Marketplace
 * public metrics and reviews.
 *
 * The collector intentionally stores the raw Marketplace statistics alongside
 * normalized fields. That prevents us from turning an ambiguous Marketplace
 * counter into a stronger claim than Microsoft actually returned.
 */

import fs from 'node:fs/promises';
import path from 'node:path';

const extensionId = process.env.MARKETPLACE_EXTENSION_ID || 'TechCodinz.ckb-vscode';
const [publisher, extensionName] = extensionId.split('.', 2);

if (!publisher || !extensionName) {
  throw new Error(`Invalid MARKETPLACE_EXTENSION_ID: ${extensionId}`);
}

const base = 'https://marketplace.visualstudio.com/_apis/public/gallery';
const apiVersion = '7.2-preview.1';
const now = new Date();
const day = now.toISOString().slice(0, 10);
const evidenceDir = path.resolve('evidence/marketplace');

const headers = {
  'content-type': 'application/json',
  accept: `application/json;api-version=${apiVersion};excludeUrls=true`,
  'user-agent': 'CKB-Marketplace-Evidence/1.0 (+https://github.com/TechCodinz/CKB)',
};

async function fetchJson(url, init = {}) {
  const response = await fetch(url, {
    ...init,
    headers: { ...headers, ...(init.headers || {}) },
  });
  if (!response.ok) {
    const body = await response.text().catch(() => '');
    throw new Error(`${response.status} ${response.statusText}: ${body.slice(0, 300)}`);
  }
  return response.json();
}

async function queryExtension() {
  const payload = {
    filters: [
      {
        criteria: [{ filterType: 7, value: extensionId }],
        pageNumber: 1,
        pageSize: 1,
        sortBy: 0,
        sortOrder: 0,
      },
    ],
    assetTypes: [],
    // Includes versions, version properties, statistics, latest version and metadata.
    flags: 2151,
  };

  const json = await fetchJson(`${base}/extensionquery`, {
    method: 'POST',
    body: JSON.stringify(payload),
  });

  const extension = json?.results?.[0]?.extensions?.[0];
  if (!extension) {
    throw new Error(`Marketplace returned no extension for ${extensionId}`);
  }
  return extension;
}

async function queryReviewsBestEffort() {
  const encodedPublisher = encodeURIComponent(publisher);
  const encodedExtension = encodeURIComponent(extensionName);
  const candidates = [
    `${base}/publishers/${encodedPublisher}/extensions/${encodedExtension}/reviews?count=100&filterOptions=0&api-version=${apiVersion}`,
    `${base}/publishers/${encodedPublisher}/vsextensions/${encodedExtension}/reviews?count=100&filterOptions=0&api-version=${apiVersion}`,
  ];

  const errors = [];
  for (const url of candidates) {
    try {
      const json = await fetchJson(url, { method: 'GET' });
      return {
        available: true,
        source_url: url,
        total_review_count: json?.totalReviewCount ?? json?.count ?? null,
        has_more_reviews: json?.hasMoreReviews ?? null,
        reviews: (json?.reviews || json?.value || []).map((review) => ({
          id: review.id ?? null,
          rating: review.rating ?? null,
          title: review.title ?? null,
          text: review.text ?? null,
          updated_date: review.updatedDate ?? null,
          product_version: review.productVersion ?? null,
          user_display_name: review.userDisplayName ?? null,
          publisher_reply: review.reply ?? review.adminReply ?? null,
        })),
      };
    } catch (error) {
      errors.push(String(error?.message || error));
    }
  }

  return {
    available: false,
    source_url: null,
    total_review_count: null,
    has_more_reviews: null,
    reviews: [],
    note: 'Public Marketplace review retrieval was unavailable on this run; rating statistics are still preserved from extension metadata.',
    errors,
  };
}

function statisticsMap(extension) {
  return Object.fromEntries(
    (extension.statistics || []).map((item) => [item.statisticName, item.value]),
  );
}

function firstNumber(stats, names) {
  for (const name of names) {
    const value = stats[name];
    if (typeof value === 'number' && Number.isFinite(value)) return value;
  }
  return null;
}

function latestVersion(extension) {
  return extension?.versions?.[0]?.version ?? null;
}

async function writeJson(file, value) {
  await fs.writeFile(file, `${JSON.stringify(value, null, 2)}\n`, 'utf8');
}

async function updateHistory(snapshot) {
  const historyPath = path.join(evidenceDir, 'history.jsonl');
  let rows = [];
  try {
    const text = await fs.readFile(historyPath, 'utf8');
    rows = text
      .split('\n')
      .filter(Boolean)
      .map((line) => JSON.parse(line));
  } catch (error) {
    if (error?.code !== 'ENOENT') throw error;
  }

  const compact = {
    date: snapshot.date,
    captured_at_utc: snapshot.captured_at_utc,
    extension_id: snapshot.extension_id,
    version: snapshot.version,
    install_count: snapshot.metrics.install_count,
    web_download_count: snapshot.metrics.web_download_count,
    average_rating: snapshot.metrics.average_rating,
    rating_count: snapshot.metrics.rating_count,
    public_review_count: snapshot.feedback.total_review_count,
  };

  rows = rows.filter((row) => row.date !== snapshot.date);
  rows.push(compact);
  rows.sort((a, b) => String(a.date).localeCompare(String(b.date)));

  await fs.writeFile(
    historyPath,
    `${rows.map((row) => JSON.stringify(row)).join('\n')}\n`,
    'utf8',
  );
}

const extension = await queryExtension();
const feedback = await queryReviewsBestEffort();
const stats = statisticsMap(extension);

const snapshot = {
  schema: 'ckb-vscode-marketplace-evidence-v1',
  date: day,
  captured_at_utc: now.toISOString(),
  source: {
    provider: 'Microsoft Visual Studio Marketplace',
    extension_query: `${base}/extensionquery`,
    marketplace_item: `https://marketplace.visualstudio.com/items?itemName=${extensionId}`,
  },
  extension_id: extensionId,
  publisher: extension?.publisher?.publisherName ?? publisher,
  extension_name: extension.extensionName ?? extensionName,
  display_name: extension.displayName ?? null,
  version: latestVersion(extension),
  published_date: extension.publishedDate ?? null,
  release_date: extension.releaseDate ?? null,
  last_updated: extension.lastUpdated ?? null,
  metrics: {
    // Public extension-query statistics normally expose install/rating signals.
    // Web-download is kept separate and remains null if Microsoft did not return it.
    install_count: firstNumber(stats, ['install', 'installCount']),
    web_download_count: firstNumber(stats, ['webDownloadCount', 'webdownload', 'downloadCount', 'download']),
    average_rating: firstNumber(stats, ['averagerating', 'averageRating']),
    rating_count: firstNumber(stats, ['ratingcount', 'ratingCount']),
    raw_statistics: stats,
  },
  feedback,
  interpretation: {
    evidence_use: [
      'dated Marketplace adoption snapshots',
      'rating and review history',
      'release-to-adoption growth evidence',
    ],
    cautions: [
      'Install count and web-download count are distinct signals and must not be conflated.',
      'A missing public web-download value does not mean zero downloads.',
      'Marketplace ratings/reviews are external recognition signals but should be presented with their dates and source.',
    ],
  },
};

await fs.mkdir(evidenceDir, { recursive: true });
await writeJson(path.join(evidenceDir, `${day}.json`), snapshot);
await writeJson(path.join(evidenceDir, 'latest.json'), snapshot);
await updateHistory(snapshot);

console.log(JSON.stringify(snapshot, null, 2));
