package org.commercenative.lucenedirect;

import org.apache.lucene.analysis.standard.StandardAnalyzer;
import org.apache.lucene.document.*;
import org.apache.lucene.index.*;
import org.apache.lucene.search.*;
import org.apache.lucene.store.Directory;
import org.apache.lucene.store.NIOFSDirectory;
import org.apache.lucene.util.BytesRef;

import java.io.BufferedReader;
import java.io.FileWriter;
import java.io.IOException;
import java.io.PrintWriter;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.util.*;

/**
 * Phase 6C (Issue #21 Phase 6, extending 6A/6B): raw Apache Lucene direct
 * baseline against the real WANDS catalog -- the same dataset, same 7
 * depth-1 category checkpoints, same field set, same operation classes
 * (filter, facet-scan, sort, deep pagination, numeric-range filter) that
 * P6A-E00/P6B-E00 already measured against Solr and commerce-native. See
 * PHASE6C_DECISION.md for why this exists: every prior cross-engine
 * comparison in this project used Solr as the ONLY generic-engine
 * baseline; this isolates Solr's own HTTP/schema/facet-wrapper overhead
 * from the underlying Lucene core.
 *
 * Timing convention matches p6a_e00_wands_vs_native_eval.rs exactly:
 * WARMUP=5, REPS=30, PAGE_SIZE=24.
 *
 * Correctness gate: every filter/range count is cross-checked against
 * the real, currently-running Solr wands_bench core (same catalog, same
 * fields) via a plain HTTP GET before any timing claim is trusted --
 * matching Phase 6A's own "correctness before speed" discipline.
 *
 * Usage: java -jar lucene-direct-bench.jar <catalog.jsonl> [solr_base_url]
 */
public class WandsLuceneBench {

    static final int WARMUP = 5;
    static final int REPS = 30;
    static final int PAGE_SIZE = 24;
    static final double RATING_THRESHOLD = 4.0;

    static final String[] DEPTH1_CHECKPOINTS = {
        "Rugs", "Storage & Organization", "Lighting", "Outdoor",
        "Décor & Pillows", "Home Improvement", "Furniture"
    };

    record Product(String id, String title, String description, String productClass,
                    String categoryDepth1, String color, String style,
                    Double averageRating) {}

    public static void main(String[] args) throws Exception {
        if (args.length < 1) {
            System.err.println("Usage: WandsLuceneBench <catalog.jsonl> [solr_base_url]");
            System.exit(1);
        }
        Path catalogPath = Paths.get(args[0]);
        String solrBaseUrl = args.length > 1 ? args[1] : "http://localhost:8983/solr/wands_bench";

        System.out.println("=== P6C-E00: raw Apache Lucene direct baseline (Issue #21 Phase 6C) ===");
        System.out.println("catalog=" + catalogPath + " solr_base_url=" + solrBaseUrl);

        List<Product> products = loadCatalog(catalogPath);
        System.out.println("loaded " + products.size() + " real WANDS products");

        Path indexDir = Files.createTempDirectory("lucene-direct-wands");
        long buildStartNs = System.nanoTime();
        buildIndex(indexDir, products);
        double buildMs = (System.nanoTime() - buildStartNs) / 1_000_000.0;
        System.out.printf("index built in %.1fms at %s%n", buildMs, indexDir);

        try (Directory dir = new NIOFSDirectory(indexDir);
             IndexReader reader = DirectoryReader.open(dir)) {
            IndexSearcher searcher = new IndexSearcher(reader);

            StringBuilder csv = new StringBuilder();
            csv.append("operation,checkpoint,candidates,lucene_p50_ms,lucene_mean_ms,solr_count,count_match\n");

            System.out.println("\n--- correctness + timing per checkpoint ---");
            for (String checkpoint : DEPTH1_CHECKPOINTS) {
                Query filter = new TermQuery(new Term("category_depth_1", checkpoint));

                // 1. Filter-only.
                long candidates = searcher.count(filter);
                double[] samples = timeReps(() -> { searcher.count(filter); });
                long solrFilterCount = solrCount(solrBaseUrl, "category_depth_1:\"" + checkpoint + "\"");
                report(csv, "filter_only", checkpoint, candidates, samples, solrFilterCount);

                // 2. product_class facet-scan under category filter. Sanity
                // check before trusting timing: bucket-count sum must never
                // exceed the filter's own candidate count (docs missing a
                // product_class value are legitimately excluded, matching
                // Solr's own facet-excludes-missing-value convention -- the
                // same non-bug pattern Phase 5/6A already documented).
                Map<String, Integer> productClassFacet = facetScan(searcher, reader, filter, "product_class_dv");
                long pcSum = productClassFacet.values().stream().mapToLong(Integer::longValue).sum();
                if (pcSum > candidates) {
                    throw new IllegalStateException("product_class facet sum " + pcSum
                        + " exceeds candidate count " + candidates + " for " + checkpoint + " -- real bug");
                }
                double[] facetSamples = timeReps(() -> { facetScan(searcher, reader, filter, "product_class_dv"); });
                report(csv, "product_class_facet_under_category", checkpoint, candidates, facetSamples, -1);

                // 3. color facet-scan under category filter, same sanity check.
                Map<String, Integer> colorFacet = facetScan(searcher, reader, filter, "color_dv");
                long colorSum = colorFacet.values().stream().mapToLong(Integer::longValue).sum();
                if (colorSum > candidates) {
                    throw new IllegalStateException("color facet sum " + colorSum
                        + " exceeds candidate count " + candidates + " for " + checkpoint + " -- real bug");
                }
                double[] colorFacetSamples = timeReps(() -> { facetScan(searcher, reader, filter, "color_dv"); });
                report(csv, "color_facet_under_category", checkpoint, candidates, colorFacetSamples, -1);

                // 4. sort by title_sort asc, top PAGE_SIZE.
                Sort titleSort = new Sort(new SortField("title_sort", SortField.Type.STRING));
                double[] titleSortSamples = timeReps(() -> { searcher.search(filter, PAGE_SIZE, titleSort); });
                report(csv, "sort_title_asc", checkpoint, candidates, titleSortSamples, -1);

                // 5. sort by average_rating desc, top PAGE_SIZE.
                Sort ratingSort = new Sort(new SortField("average_rating_dv", SortField.Type.DOUBLE, true));
                double[] ratingSortSamples = timeReps(() -> { searcher.search(filter, PAGE_SIZE, ratingSort); });
                report(csv, "sort_rating_desc", checkpoint, candidates, ratingSortSamples, -1);

                // 6. deep pagination (only if candidate set is large enough).
                if (candidates > PAGE_SIZE * 2L) {
                    int deepOffset = (int) (candidates / 2);
                    double[] pageSamples = timeReps(() -> {
                        TopDocs top = searcher.search(filter, deepOffset + PAGE_SIZE);
                        ScoreDoc[] hits = top.scoreDocs;
                        int from = Math.min(deepOffset, hits.length);
                        int to = Math.min(deepOffset + PAGE_SIZE, hits.length);
                        if (from < to) {
                            // Touch the slice so it isn't optimized away and to
                            // mirror native/Solr's own "materialize the page" cost.
                            for (int i = from; i < to; i++) reader.storedFields().document(hits[i].doc);
                        }
                    });
                    report(csv, "deep_pagination", checkpoint, candidates, pageSamples, -1);
                }
            }

            // 7. numeric-range filter (average_rating >= threshold), whole catalog,
            // matching P6B-E00's own threshold and scope (no category filter).
            Query rangeQuery = DoublePoint.newRangeQuery("average_rating_pt", RATING_THRESHOLD, Double.MAX_VALUE);
            long rangeCandidates = searcher.count(rangeQuery);
            double[] rangeSamples = timeReps(() -> { searcher.count(rangeQuery); });
            long solrRangeCount = solrCount(solrBaseUrl, "average_rating:[" + RATING_THRESHOLD + " TO *]");
            report(csv, "numeric_range_rating", "whole_catalog", rangeCandidates, rangeSamples, solrRangeCount);

            Path artifactsDir = Paths.get("docs/research/artifacts/p6c_e00_lucene_direct_run1");
            Files.createDirectories(artifactsDir);
            try (PrintWriter w = new PrintWriter(new FileWriter(artifactsDir.resolve("results.csv").toFile()))) {
                w.print(csv);
            }
            System.out.println("\nartifacts written to " + artifactsDir);
        }
    }

    static void report(StringBuilder csv, String operation, String checkpoint, long candidates,
                        double[] samplesMs, long solrCount) {
        double[] sorted = samplesMs.clone();
        Arrays.sort(sorted);
        double p50 = sorted[sorted.length / 2];
        double mean = Arrays.stream(sorted).average().orElse(0.0);
        String match = solrCount < 0 ? "n/a" : String.valueOf(solrCount == candidates);
        System.out.printf(Locale.ROOT, "  %-32s %-20s candidates=%-8d p50=%.4fms mean=%.4fms solr_count=%s match=%s%n",
            operation, checkpoint, candidates, p50, mean, solrCount < 0 ? "n/a" : String.valueOf(solrCount), match);
        csv.append(String.format(Locale.ROOT, "%s,%s,%d,%.4f,%.4f,%s,%s%n",
            operation, checkpoint, candidates, p50, mean, solrCount < 0 ? "" : String.valueOf(solrCount), match));
    }

    /** A timed operation that may throw IOException; its return value is discarded. */
    interface TimedOp {
        void run() throws IOException;
    }

    static double[] timeReps(TimedOp op) {
        try {
            for (int i = 0; i < WARMUP; i++) op.run();
            double[] samples = new double[REPS];
            for (int i = 0; i < REPS; i++) {
                long start = System.nanoTime();
                op.run();
                samples[i] = (System.nanoTime() - start) / 1_000_000.0;
            }
            return samples;
        } catch (IOException e) {
            throw new RuntimeException(e);
        }
    }

    /**
     * O(|candidates|) facet-scan: collect every matching doc id per segment
     * (no scoring), then tally its sorted-doc-value for {@code dvField}.
     * The direct Lucene analog of native's own facet_counts_by_scan and
     * Solr's facet.field bucket counting -- same operation class, no
     * mature convenience API needed since only counting is required.
     */
    static Map<String, Integer> facetScan(IndexSearcher searcher, IndexReader reader, Query filter, String dvField)
            throws IOException {
        Map<String, Integer> counts = new HashMap<>();
        for (LeafReaderContext ctx : reader.leaves()) {
            SortedDocValues dv = ctx.reader().getSortedDocValues(dvField);
            if (dv == null) continue;
            List<Integer> matchingDocs = new ArrayList<>();
            Weight weight = searcher.createWeight(searcher.rewrite(filter), ScoreMode.COMPLETE_NO_SCORES, 1f);
            Scorer scorer = weight.scorer(ctx);
            if (scorer == null) continue;
            DocIdSetIterator it = scorer.iterator();
            int doc;
            while ((doc = it.nextDoc()) != DocIdSetIterator.NO_MORE_DOCS) {
                matchingDocs.add(doc);
            }
            for (int localDoc : matchingDocs) {
                if (dv.advanceExact(localDoc)) {
                    BytesRef val = dv.lookupOrd(dv.ordValue());
                    counts.merge(val.utf8ToString(), 1, Integer::sum);
                }
            }
        }
        return counts;
    }

    static void buildIndex(Path indexDir, List<Product> products) throws IOException {
        try (Directory dir = new NIOFSDirectory(indexDir)) {
            IndexWriterConfig cfg = new IndexWriterConfig(new StandardAnalyzer());
            cfg.setOpenMode(IndexWriterConfig.OpenMode.CREATE);
            try (IndexWriter writer = new IndexWriter(dir, cfg)) {
                for (Product p : products) {
                    Document doc = new Document();
                    doc.add(new StringField("id", p.id(), Field.Store.YES));
                    doc.add(new TextField("title", p.title(), Field.Store.YES));
                    if (p.description() != null) {
                        doc.add(new TextField("description", p.description(), Field.Store.NO));
                    }
                    doc.add(new SortedDocValuesField("title_sort", new BytesRef(p.title())));

                    if (p.categoryDepth1() != null && !p.categoryDepth1().isEmpty()) {
                        doc.add(new StringField("category_depth_1", p.categoryDepth1(), Field.Store.NO));
                    }
                    if (p.productClass() != null && !p.productClass().isEmpty()) {
                        doc.add(new StringField("product_class", p.productClass(), Field.Store.NO));
                        doc.add(new SortedDocValuesField("product_class_dv", new BytesRef(p.productClass())));
                    }
                    if (p.color() != null && !p.color().isEmpty()) {
                        doc.add(new StringField("color", p.color(), Field.Store.NO));
                        doc.add(new SortedDocValuesField("color_dv", new BytesRef(p.color())));
                    }
                    if (p.style() != null && !p.style().isEmpty()) {
                        doc.add(new StringField("style", p.style(), Field.Store.NO));
                        doc.add(new SortedDocValuesField("style_dv", new BytesRef(p.style())));
                    }
                    if (p.averageRating() != null) {
                        doc.add(new DoublePoint("average_rating_pt", p.averageRating()));
                        doc.add(new NumericDocValuesField("average_rating_dv",
                            Double.doubleToRawLongBits(p.averageRating())));
                    }
                    writer.addDocument(doc);
                }
                writer.commit();
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

    // Minimal, dependency-free JSONL line parser scoped to this dataset's
    // known flat string/number/null fields -- avoids adding a JSON library
    // dependency for a one-shot ingestion of a fixed, already-validated
    // real dataset (dataset_cache/wands/catalog.jsonl, the same file
    // scripts/datasets/solr_index_wands.py and crates/phase6a-eval read).
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
            System.err.println("Solr cross-check failed for fq=" + fq + ": " + e);
            return -1;
        }
    }
}
