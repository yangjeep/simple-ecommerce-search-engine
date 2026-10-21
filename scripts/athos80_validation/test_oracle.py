#!/usr/bin/env python3
"""Hand-calculated expected outputs; tests do not call the native engine."""
import unittest
from prepare_fixtures import fixtures
from oracle import search, ScanCatalog

class HandCalculatedOracleTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.catalog, cases, cls.stress, stress_cases = fixtures()
        cls.requests={c['id']:c['request'] for c in cases}
        cls.stress_requests={c['id']:c['request'] for c in stress_cases}
        cls.high=2**32+7
        cls.prepared=ScanCatalog(cls.catalog)
    def result(self,name):return search(self.catalog,self.requests[name],prepared=self.prepared)
    def ids(self,result):return [(h['product_id'],h['selected_variant_id']) for h in result['hits']]
    def counts(self,result,name):return next(f['counts'] for f in result['facets'] if f['name']==name)
    def test_materialized_scan_matches_uncached_scan(self):
        for name in ['empty_browse_products','red_oos_blue_sibling','conditional_hide_active','variant_pin_exact_display','losing_variant_pin_keeps_best','neutral_inverse_pairs_10']:
            with self.subTest(name=name):self.assertEqual(self.result(name),search(self.catalog,self.requests[name]))
    def test_browse_and_product_distinct_facets(self):
        r=self.result('empty_browse_products')
        self.assertEqual(r['num_found'],6)
        self.assertEqual(r['eligible_variants'],8)
        self.assertEqual(self.ids(r),[(1,12),(2,21),(3,31),(4,41),(self.high,self.high*10),(2**64-1,2**64-1)])
        self.assertEqual(self.counts(r,'color'),{'blue':2,'green':1,'red':4})
        self.assertEqual(self.counts(r,'tags'),{'light':1,'sale':1})
    def test_variant_counts(self):
        r=self.result('empty_browse_variants')
        self.assertEqual(r['num_found'],8)
        self.assertEqual(self.counts(r,'color'),{'blue':2,'green':2,'red':4})
        self.assertEqual(self.counts(r,'tags'),{'light':2,'sale':2})
    def test_same_variant_filters_and_self_exclusion(self):
        r=self.result('red_oos_blue_sibling')
        self.assertEqual(self.ids(r),[(3,31),(self.high,self.high*10)])
        self.assertEqual(self.counts(r,'color'),{'blue':1,'green':1,'red':2})
        self.assertEqual(self.counts(r,'size'),{'large':2,'small':2})
    def test_variant_text_override_and_no_recall_creation(self):
        self.assertEqual(self.result('variant_overrides_product_text')['num_found'],0)
        self.assertEqual(self.ids(self.result('variant_text_recall')),[(3,31)])
        self.assertEqual(self.result('personal_cannot_create_recall')['num_found'],0)
        self.assertEqual(self.result('enum_not_lexical_text')['num_found'],0)
    def test_signed_zero_single_facet_value(self):
        r=self.result('numeric_signed_zero_facets')
        self.assertEqual(self.ids(r),[(1,12),(2,21)])
        self.assertEqual(self.counts(r,'weight'),{'0':2,'1.5':1,'2':1,'3':1})
    def test_lexical_bucket_dominates_later_layers(self):
        self.assertEqual(self.ids(self.result('bucket_dominates_merch_personal'))[:2],[(2,21),(3,31)])
    def test_ordered_axes_and_personal_not_additive_merch(self):
        self.assertEqual(self.ids(self.result('ordered_merch_axes'))[:2],[(2,21),(3,31)])
        self.assertEqual(self.ids(self.result('personal_inside_group'))[:2],[(2,21),(3,31)])
    def test_scope_override_is_whole_set(self):
        r=self.result('whole_group_scope_replacement')
        self.assertEqual(self.ids(r)[:3],[(1,12),(2,21),(3,31)])
        self.assertTrue(all(h['merch_groups']==[0] for h in r['hits']))
    def test_conditional_hides(self):
        self.assertEqual(self.result('conditional_hide_inactive')['num_found'],6)
        r=self.result('conditional_hide_active')
        self.assertEqual(r['num_found'],4)
        self.assertEqual([p for p,v in self.ids(r)],[1,4,self.high,2**64-1])
        self.assertEqual(self.counts(r,'color'),{'blue':1,'green':1,'red':2})
    def test_pin_exact_variant_and_losing_pin(self):
        self.assertEqual(self.ids(self.result('variant_pin_exact_display'))[0],(2,22))
        self.assertEqual(self.ids(self.result('product_pin_best_display'))[0],(2,21))
        r=self.result('losing_variant_pin_keeps_best')
        self.assertEqual(self.ids(r)[:3],[(3,31),(1,12),(2,21)])
        self.assertEqual(r['hits'][0]['pin_rule_id'],'a-other')
        self.assertIsNone(r['hits'][2]['pin_rule_id'])
    def test_hidden_and_oos_pins_do_not_resurrect(self):
        self.assertNotIn(2,[p for p,v in self.ids(self.result('hide_beats_pin'))])
        self.assertEqual(self.result('ineligible_pins_ignored')['num_found'],4)
        with self.assertRaises(ValueError):self.result('eligible_pin_outside_is_error')
    def test_personal_bounds_and_order(self):
        for name,factor in [('factor_lower_bound_saturation',.25),('factor_upper_bound_saturation',4.0),('factor_multiply_clamp_once',4.0),('factor_order_invariant',4.0)]:
            r=self.result(name)
            hit=next(h for h in r['hits'] if h['product_id']==3)
            self.assertAlmostEqual(hit['personal_factor'],factor)
    def test_neutral_inverse_pairs_do_not_change_ranking(self):
        base=self.ids(self.result('neutral_missing_personal'))
        for count in [3,10]:
            r=self.result(f'neutral_inverse_pairs_{count}')
            self.assertEqual(self.ids(r),base)
            self.assertEqual(next(h['personal_factor'] for h in r['hits'] if h['product_id']==3),1.0)
    def test_score_roundtrip_bits_and_signed_zero(self):
        import struct
        r=self.result('binary64_score_roundtrip')
        h=next(h for h in r['hits'] if h['product_id']==1)
        self.assertEqual(struct.pack('!d',h['lexical_score']),struct.pack('!d',9.842065267857133e-06))
        r=self.result('signed_score_zero_canonical')
        h=next(h for h in r['hits'] if h['product_id']==1)
        self.assertEqual(struct.pack('!d',h['lexical_score']),struct.pack('!d',0.0))
    def test_shopper_sort_replaces_rank_and_pins(self):
        r=self.result('explicit_shopper_sort')
        self.assertEqual([p for p,v in self.ids(r)],[3,4,2**64-1,self.high,1,2])
        self.assertTrue(all(h['personal_factor']==1.0 and h['pin_rule_id'] is None for h in r['hits']))
    def test_global_topk_and_late_merch(self):
        r=search(self.stress,self.stress_requests['global_rank_before_group_topk'])
        self.assertEqual(self.ids(r),[(100,1000),(200,9999)])
        r=search(self.stress,self.stress_requests['late_merch_boost_not_presliced'])
        self.assertEqual(self.ids(r),[(200,9999)])

if __name__=='__main__':unittest.main()
