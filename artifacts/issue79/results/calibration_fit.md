## Calibration phase-time medians (in-process us, median of 3 runs)

| cell | family | |C| | V | x=|C|/V or y | legacy | path A | path B | A/B |
|---|---|---|---|---|---|---|---|---|
| fc1_full_material | facet | 515928 | 162 | 3.18e+03 | 7448 | 6742 | 973 | 6.93 |
| fc2_full_shape | facet | 515928 | 94 | 5.49e+03 | 7540 | 6721 | 671 | 10 |
| fc3_style_modern_x_color | facet | 97188 | 2825 | 34.4 | 16820 | 3234 | 9958 | 0.325 |
| fc4_color_white_x_style | facet | 17256 | 65 | 265 | 5280 | 725 | 4044 | 0.179 |
| fc5_color_white_x_primarymaterial | facet | 17256 | 244 | 70.7 | 13541 | 788 | 10320 | 0.0763 |
| fc6_depth3_x_shape | facet | 36 | 94 | 0.383 | 1213 | 18 | 1049 | 0.0171 |
| fc7_pergolas_x_color | facet | 156 | 2825 | 0.0552 | 16734 | 31 | 12174 | 0.00253 |
| sc1_full_rating_count_desc | sort | 515928 | - | 1.07e+04 | 1191061 | 12428 | 168 | 73.9 |
| sc2_style_modern_average_rating_asc | sort | 97188 | - | 381 | 269861 | 3453 | 98 | 35.1 |
| sc3_accent_chairs_review_count_desc | sort | 13236 | - | 7.07 | 40291 | 1188 | 549 | 2.16 |
| sc4_color_black_rating_count_asc | sort | 16332 | - | 10.8 | 51752 | 1303 | 289 | 4.51 |
| sc5_pergolas_average_rating_desc | sort | 156 | - | 0.000983 | 671 | 156 | 10464 | 0.0149 |
| sc6_depth5_review_count_desc | sort | 12 | - | 5.81e-06 | 77 | 32 | 12144 | 0.00265 |

### facet: ordinal (A) wins >=25% on ['fc3_style_modern_x_color', 'fc4_color_white_x_style', 'fc5_color_white_x_primarymaterial', 'fc6_depth3_x_shape', 'fc7_pergolas_x_color']; bitmap (B) wins >=25% on ['fc1_full_material', 'fc2_full_shape'] -> crossover=True
    summed calibration time: all-A=18259us all-B=39190us best-threshold=919.4972390657695 -> 6439us
    CROSSOVER: fixed threshold = 919.4972390657695

### sort: candidate top-K (A) wins >=25% on ['sc5_pergolas_average_rating_desc', 'sc6_depth5_review_count_desc']; presorted (B) wins >=25% on ['sc1_full_rating_count_desc', 'sc2_style_modern_average_rating_asc', 'sc3_accent_chairs_review_count_desc', 'sc4_color_black_rating_count_asc'] -> crossover=True
    summed calibration time: all-A=18560us all-B=23712us best-threshold=0.0833779131971903 -> 1292us
    CROSSOVER: fixed threshold = 0.0833779131971903

{"tau_f": 919.4972390657695, "rho_s": 0.0833779131971903}
