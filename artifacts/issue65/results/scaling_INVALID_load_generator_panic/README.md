INVALID: every point is a harness crash, not a measurement.
i65_load panicked while building targets for class F under treatment "native", although the native_plp mix never schedules F. No result JSON was produced, and run_i65.sh reported FAIL for the missing files. Fixed with an inert target plus an assertion. Rerun under results/scaling/.
