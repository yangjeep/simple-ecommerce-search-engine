package org.commercenative.opensearchdirect;

import org.opensearch.action.bulk.BulkRequestBuilder;
import org.opensearch.action.bulk.BulkResponse;
import org.opensearch.action.search.SearchResponse;
import org.opensearch.index.query.QueryBuilder;
import org.opensearch.index.query.QueryBuilders;
import org.opensearch.search.aggregations.AggregationBuilders;
import org.opensearch.search.aggregations.bucket.terms.Terms;
import org.opensearch.search.aggregations.bucket.terms.TermsAggregationBuilder;
import org.opensearch.search.builder.SearchSourceBuilder;
import org.opensearch.search.sort.SortOrder;
import org.opensearch.test.OpenSearchSingleNodeTestCase;
import org.opensearch.core.xcontent.XContentBuilder;
import org.opensearch.common.xcontent.XContentFactory;

import java.io.BufferedReader;
import java.io.IOException;
import java.io.PrintWriter;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.List;
import java.util.Locale;
import java.util.Map;

/**
 * P6E-E01 (Issue #21 Phase 6E, extending 6A/6B/6C/6D): a genuine embedded
 * Elasticsearch baseline against the real WANDS catalog -- the fourth
 * generic-engine baseline this project has built (Solr, then raw Lucene
 * direct in Phase 6C), and the first actual Elasticsearch data point in
 * this entire research campaign.
 *
 * Phase 6C's cross-engine audit concluded Elasticsearch was "genuinely
 * blocked" in this environment, but only tested two routes: the official
 * prebuilt distribution (artifacts.elastic.co, 403) and a from-source
 * build (also blocked). It never tried fetching the actual server
 * library + org.opensearch.test:framework jars directly from Maven
 * Central, the same trick that made P6C-E00's raw-Lucene benchmark
 * possible. That route resolves cleanly in this environment (see
 * PHASE6E_LOG.md for the exact `mvn dependency:get` proof), and --
 * unlike the from-source build -- gets all the way to a real, running,
 * single-node embedded cluster once two concrete, disclosed classpath/
 * JVM issues are fixed (see this module's pom.xml comments): a jar-hell
 * conflict between JUnit's transitive hamcrest-core and the framework's
 * own hamcrest, and JDK 17+'s default refusal to install a legacy
 * SecurityManager without -Djava.security.manager=allow.
 *
 * This class runs the same operation classes and same 7 real WANDS
 * depth-1 category checkpoints P6A-E00/P6C-E00 already used against
 * Solr and raw Lucene, cross-checked against the same live Solr
 * wands_bench core before any timing claim is trusted -- matching this
 * whole project's "correctness before speed" discipline.
 *
 * Run via: mvn test -Dcatalog.path=... -Dsolr.base.url=... (system
 * properties, not argv -- Surefire/JUnit controls the entry point here,
 * not a plain main()).
 */
public class WandsOpenSearchBenchTest extends OpenSearchSingleNodeTestCase {

    static final int WARMUP = 5;
    static final int REPS = 30;
    static final int PAGE_SIZE = 24;
    static final double RATING_THRESHOLD = 4.0;
    static final String INDEX = "wands_bench_opensearch";

    static final String[] DEPTH1_CHECKPOINTS = {
        "Rugs", "Storage & Organization", "Lighting", "Outdoor",
        "Décor & Pillows", "Home Improvement", "Furniture"
    };

    record Product(String id, String title, String description, String productClass,
                    String categoryDepth1, String color, String style,
                    Double averageRating) {}

    public void testWandsBenchmark() throws Exception {
        String repoRoot = System.getProperty("repo.root", "..");
        String catalogPath = System.getProperty("catalog.path", repoRoot + "/dataset_cache/wands/catalog.jsonl");
        String solrBaseUrl = System.getProperty("solr.base.url", "http://localhost:8983/solr/wands_bench");

        logger.info("=== P6E-E01: embedded OpenSearch direct baseline (Issue #21 Phase 6E) ===");
        logger.info("catalog={} solr_base_url={}", catalogPath, solrBaseUrl);

        List<Product> products = loadCatalog(Paths.get(catalogPath));
        logger.info("loaded {} real WANDS products", products.size());

        createIndex(INDEX, org.opensearch.common.settings.Settings.builder()
            .put("number_of_shards", 1)
            .put("number_of_replicas", 0)
            .build());
        client().admin().indices().preparePutMapping(INDEX).setSource(mapping()).get();

        long buildStartNs = System.nanoTime();
        bulkIndex(products);
        client().admin().indices().prepareRefresh(INDEX).get();
        double buildMs = (System.nanoTime() - buildStartNs) / 1_000_000.0;
        logger.info("index built in {}ms", String.format(Locale.ROOT, "%.1f", buildMs));

        StringBuilder csv = new StringBuilder();
        csv.append("operation,checkpoint,candidates,es_p50_ms,es_mean_ms,solr_count,count_match\n");

        for (String checkpoint : DEPTH1_CHECKPOINTS) {
            QueryBuilder filter = QueryBuilders.termQuery("category_depth_1", checkpoint);

            // 1. Filter-only (count).
            long candidates = count(filter);
            double[] samples = timeReps(() -> count(filter));
            long solrFilterCount = solrCount(solrBaseUrl, "category_depth_1:\"" + checkpoint + "\"");
            report(csv, "filter_only", checkpoint, candidates, samples, solrFilterCount);

            // 2. product_class facet (terms agg) under category filter.
            // Sanity check before trusting timing: bucket-sum must never
            // exceed the filter's own candidate count (docs missing
            // product_class are legitimately excluded from a terms agg,
            // matching Solr/Lucene-direct/commerce-native's own
            // facet-excludes-missing-value convention).
            Map<String, Long> productClassFacet = termsFacet(filter, "product_class");
            long pcSum = productClassFacet.values().stream().mapToLong(Long::longValue).sum();
            if (pcSum > candidates) {
                throw new IllegalStateException("product_class facet sum " + pcSum
                    + " exceeds candidate count " + candidates + " for " + checkpoint + " -- real bug");
            }
            double[] facetSamples = timeReps(() -> termsFacet(filter, "product_class"));
            report(csv, "product_class_facet_under_category", checkpoint, candidates, facetSamples, -1);

            // 3. color facet under category filter, same sanity check.
            Map<String, Long> colorFacet = termsFacet(filter, "color");
            long colorSum = colorFacet.values().stream().mapToLong(Long::longValue).sum();
            if (colorSum > candidates) {
                throw new IllegalStateException("color facet sum " + colorSum
                    + " exceeds candidate count " + candidates + " for " + checkpoint + " -- real bug");
            }
            double[] colorFacetSamples = timeReps(() -> termsFacet(filter, "color"));
            report(csv, "color_facet_under_category", checkpoint, candidates, colorFacetSamples, -1);

            // 4. sort by title (keyword) asc, top PAGE_SIZE.
            double[] titleSortSamples = timeReps(() -> client().prepareSearch(INDEX)
                .setQuery(filter).setSize(PAGE_SIZE).addSort("title_sort", SortOrder.ASC).get());
            report(csv, "sort_title_asc", checkpoint, candidates, titleSortSamples, -1);

            // 5. sort by average_rating desc, top PAGE_SIZE.
            double[] ratingSortSamples = timeReps(() -> client().prepareSearch(INDEX)
                .setQuery(filter).setSize(PAGE_SIZE).addSort("average_rating", SortOrder.DESC).get());
            report(csv, "sort_rating_desc", checkpoint, candidates, ratingSortSamples, -1);

            // 6. deep pagination (only if candidate set is large enough).
            if (candidates > PAGE_SIZE * 2L) {
                int deepOffset = (int) (candidates / 2);
                double[] pageSamples = timeReps(() -> client().prepareSearch(INDEX)
                    .setQuery(filter).setFrom(deepOffset).setSize(PAGE_SIZE).get());
                report(csv, "deep_pagination", checkpoint, candidates, pageSamples, -1);
            }
        }

        // 7. numeric-range filter (average_rating >= threshold), whole
        // catalog, matching P6B-E00/P6C-E00's own threshold and scope.
        QueryBuilder rangeQuery = QueryBuilders.rangeQuery("average_rating").gte(RATING_THRESHOLD);
        long rangeCandidates = count(rangeQuery);
        double[] rangeSamples = timeReps(() -> count(rangeQuery));
        long solrRangeCount = solrCount(solrBaseUrl, "average_rating:[" + RATING_THRESHOLD + " TO *]");
        report(csv, "numeric_range_rating", "whole_catalog", rangeCandidates, rangeSamples, solrRangeCount);

        // OpenSearch's test-framework SecurityManager only grants file access under
        // java.io.tmpdir and a few ES-internal paths -- not arbitrary repo
        // paths -- so artifacts are written there and archived into
        // docs/research/artifacts/ by the invoking shell after this JVM
        // exits, matching the same fix already applied to catalog loading.
        Path artifactsDir = Paths.get(System.getProperty("java.io.tmpdir"), "p6e_e01_opensearch_direct_run1");
        Files.createDirectories(artifactsDir);
        try (PrintWriter w = new PrintWriter(Files.newBufferedWriter(artifactsDir.resolve("results.csv"), StandardCharsets.UTF_8))) {
            w.print(csv);
        }
        logger.info("artifacts written to {}", artifactsDir);
    }

    long count(QueryBuilder q) {
        // Unlike ES 8.x's SearchResponse, OpenSearch's (forked pre-8.x) is
        // not ref-counted -- no explicit release needed here.
        SearchResponse resp = client().prepareSearch(INDEX).setQuery(q).setSize(0)
            .setTrackTotalHitsUpTo(Integer.MAX_VALUE).get();
        return resp.getHits().getTotalHits().value;
    }

    Map<String, Long> termsFacet(QueryBuilder filter, String field) {
        TermsAggregationBuilder agg = AggregationBuilders.terms("facet").field(field).size(10_000);
        SearchResponse resp = client().prepareSearch(INDEX).setQuery(filter).setSize(0).addAggregation(agg).get();
        Terms terms = resp.getAggregations().get("facet");
        Map<String, Long> counts = new java.util.HashMap<>();
        for (Terms.Bucket b : terms.getBuckets()) {
            counts.put(b.getKeyAsString(), b.getDocCount());
        }
        return counts;
    }

    interface TimedOp {
        void run();
    }

    static double[] timeReps(TimedOp op) {
        for (int i = 0; i < WARMUP; i++) op.run();
        double[] samples = new double[REPS];
        for (int i = 0; i < REPS; i++) {
            long start = System.nanoTime();
            op.run();
            samples[i] = (System.nanoTime() - start) / 1_000_000.0;
        }
        return samples;
    }

    void report(StringBuilder csv, String operation, String checkpoint, long candidates,
                double[] samplesMs, long solrCount) {
        double[] sorted = samplesMs.clone();
        Arrays.sort(sorted);
        double p50 = sorted[sorted.length / 2];
        double mean = Arrays.stream(sorted).average().orElse(0.0);
        String match = solrCount < 0 ? "n/a" : String.valueOf(solrCount == candidates);
        logger.info(String.format(Locale.ROOT, "  %-32s %-20s candidates=%-8d p50=%.4fms mean=%.4fms solr_count=%s match=%s",
            operation, checkpoint, candidates, p50, mean, solrCount < 0 ? "n/a" : String.valueOf(solrCount), match));
        csv.append(String.format(Locale.ROOT, "%s,%s,%d,%.4f,%.4f,%s,%s%n",
            operation, checkpoint, candidates, p50, mean, solrCount < 0 ? "" : String.valueOf(solrCount), match));
    }

    static XContentBuilder mapping() throws IOException {
        return XContentFactory.jsonBuilder().startObject().startObject("properties")
            .startObject("id").field("type", "keyword").endObject()
            .startObject("title").field("type", "text").endObject()
            .startObject("title_sort").field("type", "keyword").endObject()
            .startObject("description").field("type", "text").endObject()
            .startObject("category_depth_1").field("type", "keyword").endObject()
            .startObject("product_class").field("type", "keyword").endObject()
            .startObject("color").field("type", "keyword").endObject()
            .startObject("style").field("type", "keyword").endObject()
            .startObject("average_rating").field("type", "double").endObject()
            .endObject().endObject();
    }

    void bulkIndex(List<Product> products) {
        final int batchSize = 1000;
        for (int i = 0; i < products.size(); i += batchSize) {
            BulkRequestBuilder bulk = client().prepareBulk();
            int end = Math.min(i + batchSize, products.size());
            for (Product p : products.subList(i, end)) {
                Map<String, Object> src = new java.util.HashMap<>();
                src.put("id", p.id());
                src.put("title", p.title());
                src.put("title_sort", p.title());
                if (p.description() != null) src.put("description", p.description());
                if (p.categoryDepth1() != null && !p.categoryDepth1().isEmpty()) {
                    src.put("category_depth_1", p.categoryDepth1());
                }
                if (p.productClass() != null && !p.productClass().isEmpty()) {
                    src.put("product_class", p.productClass());
                }
                if (p.color() != null && !p.color().isEmpty()) src.put("color", p.color());
                if (p.style() != null && !p.style().isEmpty()) src.put("style", p.style());
                if (p.averageRating() != null) src.put("average_rating", p.averageRating());
                bulk.add(client().prepareIndex(INDEX).setId(p.id()).setSource(src));
            }
            BulkResponse resp = bulk.get();
            if (resp.hasFailures()) {
                throw new IllegalStateException("bulk index failures: " + resp.buildFailureMessage());
            }
        }
    }

    static List<Product> loadCatalog(Path path) throws IOException {
        List<Product> out = new ArrayList<>();
        try (BufferedReader r = Files.newBufferedReader(path, StandardCharsets.UTF_8)) {
            String line;
            while ((line = r.readLine()) != null) {
                if (line.isBlank()) continue;
                out.add(parseProduct(line));
            }
        }
        return out;
    }

    static Product parseProduct(String line) {
        Map<String, String> raw = MiniJson.parseFlatObject(line);
        Double rating = null;
        String ratingStr = raw.get("average_rating");
        if (ratingStr != null) {
            try {
                rating = Double.parseDouble(ratingStr);
            } catch (NumberFormatException ignored) {
            }
        }
        return new Product(
            raw.get("id"), raw.getOrDefault("title", ""), raw.get("description"),
            raw.get("product_class"), raw.get("category_depth_1"),
            raw.get("color"), raw.get("style"), rating
        );
    }

    static final org.apache.logging.log4j.Logger STATIC_LOGGER =
        org.apache.logging.log4j.LogManager.getLogger(WandsOpenSearchBenchTest.class);

    static long solrCount(String baseUrl, String fq) {
        try {
            String url = baseUrl + "/select?q=*:*&fq=" + java.net.URLEncoder.encode(fq, "UTF-8")
                + "&rows=0&wt=json";
            java.net.URI uri = java.net.URI.create(url);
            try (java.io.InputStream is = uri.toURL().openStream()) {
                String body = new String(is.readAllBytes(), StandardCharsets.UTF_8);
                int idx = body.indexOf("\"numFound\":");
                if (idx < 0) return -1;
                int start = idx + "\"numFound\":".length();
                int end = start;
                while (end < body.length() && Character.isDigit(body.charAt(end))) end++;
                return Long.parseLong(body.substring(start, end));
            }
        } catch (Exception e) {
            STATIC_LOGGER.warn("Solr cross-check failed for fq={}: {}", fq, e.toString());
            return -1;
        }
    }
}
