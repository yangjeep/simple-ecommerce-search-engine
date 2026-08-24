export const meta = {
  name: 'e2b-stability-rerun',
  description: 'Issue #42 E2b: independent repeated-run stability re-run (3 new blinded LLM passes per configuration)',
  phases: [
    { title: 'LLM proposal passes', detail: '12 independent blinded subagent calls (4 configs x 3 new runs)' },
  ],
}

// Issue #42's E2b serving-contract-closure pass, item 3: "Run a
// materially larger independent repeated-run sample using the exact
// frozen prompt, inputs, model/provider/version/settings, validation
// rules, and oracle... choose and document the run count before looking
// at the new aggregate result."
//
// Run count decided and documented HERE, before any new result is seen:
// 3 additional independent runs per configuration (bringing each
// configuration from 2 to 5 total runs), 4 configurations x 3 = 12 new
// independent subagent calls. This raises the number of pairwise
// agreement comparisons per configuration from C(2,2)=1 to C(5,2)=10 --
// a 10x increase in the stability metric's own sample size (4 -> 40
// total pairs across all 4 configurations), a materially larger sample
// by construction, not a marginal one.
//
// Inputs: `args.boundedInputs[config]` is the EXACT bounded-input data
// (shown key/alias name, real occurrences/distinct_values/uniqueness_ratio/
// numeric_parseable_fraction/mean_value_length/variant_scoped/sample_values)
// the original 8 passes were given, deterministically reconstructed by
// `crates/issue42-eval/src/bin/e2b_dump_bounded_inputs.rs` from the same
// real, committed statistics `e2b_workload`/`e2b_key_mapping` already
// provide -- never re-derived independently. The literal PROMPT WORDING
// of the original passes was never committed (a real, disclosed gap,
// analogous to the /tmp-only key-mapping gap this same pass already
// fixed); the prompt below is a faithful reconstruction from the
// protocol's own descriptor schema and instructions text
// (docs/experiments/ISSUE42_PROTOCOL.md's E2b amendment 1), not a
// byte-identical replay -- disclosed as such in the final write-up, not
// silently presented as identical.
//
// Each subagent call here is a genuinely FRESH agent with no access to
// this session's conversation history, any other pass's output, or any
// oracle mapping -- the same "cannot look," not merely "don't look,"
// blinding property the original 8 passes relied on.

const DESCRIPTOR_SCHEMA = {
  type: 'object',
  properties: {
    descriptors: {
      type: 'array',
      items: {
        type: 'object',
        properties: {
          key: { type: 'string', description: 'copy the shown_key exactly as given in the input' },
          semantic_role: { type: 'string', enum: ['identifier', 'enum', 'numeric', 'boolean', 'free_text', 'relationship', 'ignore'] },
          value_type: { type: 'string', enum: ['string', 'number', 'boolean'] },
          scope: { type: 'string', enum: ['product', 'variant', 'relationship'] },
          supported_operators: { type: 'array', items: { type: 'string', enum: ['eq', 'range', 'contains', 'exact_lookup'] } },
          aliases: { type: 'array', items: { type: 'string' } },
          relationship_semantics: { type: ['string', 'null'] },
          retrieval_significance: { type: 'string', enum: ['retrieval_significant', 'ranking_only', 'ignore'] },
          candidate_physical_primitive: { type: 'string', enum: ['bitmap_enum', 'numeric_range', 'identifier_dictionary', 'lexical_postings', 'none'] },
          confidence: { type: 'number', minimum: 0, maximum: 1 },
          evidence: { type: 'string' },
          abstain: { type: 'boolean' },
        },
        required: ['key', 'semantic_role', 'value_type', 'scope', 'supported_operators', 'aliases', 'retrieval_significance', 'candidate_physical_primitive', 'confidence', 'evidence', 'abstain'],
      },
    },
  },
  required: ['descriptors'],
}

function buildPrompt(config, fields) {
  return `You are participating in a blinded, independent feature-discovery pass for an offline commerce search-engine control-plane research experiment. You have NO access to any other pass's output, any reference/oracle mapping, or any real field names beyond exactly what is shown to you below. Do not assume this conversation has any other context -- there is none.

TASK: For each field below, propose a structural descriptor -- what semantic role it plays, what physical serving primitive it should compile into, and how confident you are -- based ONLY on the field's shown name and the real statistics/sample values given below. If you cannot confidently classify a field from this evidence alone, set "abstain": true rather than guessing.

DESCRIPTOR SCHEMA (produce exactly this shape for every field listed below):
- key: string -- copy the field's own "shown_key" exactly as given
- semantic_role: one of "identifier" | "enum" | "numeric" | "boolean" | "free_text" | "relationship" | "ignore"
- value_type: one of "string" | "number" | "boolean"
- scope: one of "product" | "variant" | "relationship" -- does this field vary per product, per variant (e.g. size/color options of the SAME product), or does it reference a DIFFERENT product/listing entirely (a cross-reference)?
- supported_operators: array drawn from "eq" | "range" | "contains" | "exact_lookup" -- which query operators does this field meaningfully support?
- aliases: array of real query phrases a shopper might type that this field would answer (e.g. "blue", "organic", "under 50 lbs") -- empty array if none apply
- relationship_semantics: string, or null -- set (non-null) ONLY when scope is "relationship"; describe what the cross-reference means in one sentence
- retrieval_significance: one of "retrieval_significant" (a shopper would filter/search directly on this) | "ranking_only" (useful as a ranking signal but not a direct filter) | "ignore" (neither)
- candidate_physical_primitive: one of "bitmap_enum" | "numeric_range" | "identifier_dictionary" | "lexical_postings" | "none"
- confidence: number from 0.0 to 1.0
- evidence: string citing the SPECIFIC real statistics/sample values above that justify your classification
- abstain: boolean -- true if you decline to classify this field at all

INPUT: real per-field statistics for the "${config}" configuration (occurrences = total raw occurrences observed in the real feed; distinct_values = number of distinct real values observed; uniqueness_ratio = distinct_values / occurrences; numeric_parseable_fraction = fraction of real sampled values that parse as a number; mean_value_length = occurrence-weighted mean character length of real values; variant_scoped = true/false/null, where null means "not applicable, this data source has no formal Variant concept at all"; sample_values = up to 5 real distinct values actually observed for this field):

${JSON.stringify(fields, null, 2)}

Return a JSON object of the shape {"descriptors": [...]} with exactly one descriptor per field listed above (every "shown_key" above must appear exactly once as a "key" in your output, in any order). This is real research data for an offline classification task -- return only the descriptors, no other commentary.`
}

phase('LLM proposal passes')

const NEW_RUN_INDICES = [3, 4, 5]
const CONFIGS = ['wands_baseline', 'wands_anonymized', 'wands_noisy', 'automotive']

const calls = []
for (const config of CONFIGS) {
  for (const runIndex of NEW_RUN_INDICES) {
    calls.push({ config, runIndex })
  }
}

log(`Launching ${calls.length} independent blinded LLM proposal passes (${CONFIGS.length} configs x ${NEW_RUN_INDICES.length} new runs each)...`)

const results = await parallel(calls.map(({ config, runIndex }) => async () => {
  const fields = args.boundedInputs[config]
  const prompt = buildPrompt(config, fields)
  const output = await agent(prompt, {
    label: `${config}_run${runIndex}`,
    schema: DESCRIPTOR_SCHEMA,
  })
  if (!output) return null
  return {
    configuration: config,
    run_index: runIndex,
    descriptors: output.descriptors,
  }
}))

const succeeded = results.filter(Boolean)
log(`${succeeded.length}/${calls.length} passes returned a valid result.`)

return {
  requested: calls.length,
  succeeded: succeeded.length,
  passes: succeeded,
  failed: calls.filter((_, i) => !results[i]).map(c => `${c.config}_run${c.runIndex}`),
}
