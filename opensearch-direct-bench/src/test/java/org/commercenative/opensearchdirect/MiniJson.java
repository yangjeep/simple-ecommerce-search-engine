package org.commercenative.opensearchdirect;

import java.util.HashMap;
import java.util.Map;

/**
 * A minimal, correct JSON-object-line parser scoped to exactly what
 * {@code dataset_cache/wands/catalog.jsonl} needs: one flat top-level
 * object per line, string/number/null values only (no nested objects or
 * arrays for the fields this benchmark reads). Not a general JSON
 * library -- deliberately narrow, to avoid adding a dependency for
 * ingesting a single, fixed, already-validated real dataset this
 * project's own Python/Rust tooling already parses correctly elsewhere.
 * Handles standard JSON string escapes (\", \\, \/, \b, \f, \n, \r, \t,
 * \\uXXXX) since WANDS titles/descriptions contain real unicode-escaped
 * punctuation (e.g. ’ apostrophes).
 */
final class MiniJson {
    private MiniJson() {}

    static Map<String, String> parseFlatObject(String line) {
        Map<String, String> out = new HashMap<>();
        int[] pos = {0};
        skipWs(line, pos);
        expect(line, pos, '{');
        skipWs(line, pos);
        if (peek(line, pos) == '}') {
            return out;
        }
        while (true) {
            skipWs(line, pos);
            String key = parseString(line, pos);
            skipWs(line, pos);
            expect(line, pos, ':');
            skipWs(line, pos);
            String value = parseValue(line, pos);
            out.put(key, value);
            skipWs(line, pos);
            char c = peek(line, pos);
            if (c == ',') {
                pos[0]++;
                continue;
            }
            if (c == '}') {
                pos[0]++;
                break;
            }
            throw new IllegalStateException("malformed JSON at " + pos[0] + " in: " + line.substring(0, Math.min(80, line.length())));
        }
        return out;
    }

    /** Returns the value's string form, or null if the JSON value was `null`. */
    private static String parseValue(String s, int[] pos) {
        char c = peek(s, pos);
        if (c == '"') {
            return parseString(s, pos);
        }
        if (c == 'n') { // null
            expectLiteral(s, pos, "null");
            return null;
        }
        if (c == 't') {
            expectLiteral(s, pos, "true");
            return "true";
        }
        if (c == 'f') {
            expectLiteral(s, pos, "false");
            return "false";
        }
        if (c == '-' || Character.isDigit(c)) {
            return parseNumber(s, pos);
        }
        if (c == '{') {
            skipBalanced(s, pos, '{', '}');
            return null;
        }
        if (c == '[') {
            skipBalanced(s, pos, '[', ']');
            return null;
        }
        throw new IllegalStateException("unexpected character '" + c + "' at " + pos[0]);
    }

    private static String parseNumber(String s, int[] pos) {
        int start = pos[0];
        while (pos[0] < s.length() && "-+.0123456789eE".indexOf(s.charAt(pos[0])) >= 0) {
            pos[0]++;
        }
        return s.substring(start, pos[0]);
    }

    private static void expectLiteral(String s, int[] pos, String literal) {
        if (!s.regionMatches(pos[0], literal, 0, literal.length())) {
            throw new IllegalStateException("expected literal '" + literal + "' at " + pos[0]);
        }
        pos[0] += literal.length();
    }

    private static void skipBalanced(String s, int[] pos, char open, char close) {
        int depth = 0;
        while (pos[0] < s.length()) {
            char c = s.charAt(pos[0]);
            if (c == '"') {
                parseString(s, pos); // consumes the string, respecting escapes
                continue;
            }
            if (c == open) depth++;
            else if (c == close) {
                depth--;
                pos[0]++;
                if (depth == 0) return;
                continue;
            }
            pos[0]++;
        }
    }

    private static String parseString(String s, int[] pos) {
        expect(s, pos, '"');
        StringBuilder sb = new StringBuilder();
        while (true) {
            char c = s.charAt(pos[0]);
            if (c == '"') {
                pos[0]++;
                break;
            }
            if (c == '\\') {
                pos[0]++;
                char esc = s.charAt(pos[0]);
                switch (esc) {
                    case '"': sb.append('"'); pos[0]++; break;
                    case '\\': sb.append('\\'); pos[0]++; break;
                    case '/': sb.append('/'); pos[0]++; break;
                    case 'b': sb.append('\b'); pos[0]++; break;
                    case 'f': sb.append('\f'); pos[0]++; break;
                    case 'n': sb.append('\n'); pos[0]++; break;
                    case 'r': sb.append('\r'); pos[0]++; break;
                    case 't': sb.append('\t'); pos[0]++; break;
                    case 'u':
                        pos[0]++;
                        String hex = s.substring(pos[0], pos[0] + 4);
                        sb.append((char) Integer.parseInt(hex, 16));
                        pos[0] += 4;
                        break;
                    default:
                        throw new IllegalStateException("bad escape \\" + esc + " at " + pos[0]);
                }
            } else {
                sb.append(c);
                pos[0]++;
            }
        }
        return sb.toString();
    }

    private static void skipWs(String s, int[] pos) {
        while (pos[0] < s.length() && Character.isWhitespace(s.charAt(pos[0]))) pos[0]++;
    }

    private static char peek(String s, int[] pos) {
        return s.charAt(pos[0]);
    }

    private static void expect(String s, int[] pos, char c) {
        if (s.charAt(pos[0]) != c) {
            throw new IllegalStateException("expected '" + c + "' at " + pos[0] + " got '" + s.charAt(pos[0]) + "'");
        }
        pos[0]++;
    }
}
