# SCORM 2004 4th Edition test suite: LMS test packages

Data from the standards body's SCORM 2004 4th Edition test suite (Advanced Distributed Learning), version 1.1.1, as published at `github.com/adlnet/SCORM-2004-4ed-Test-Suite`, commit `050f1b4282c914154ab80254e4767ce8c58d0d36`. Copyright 2014 Advanced Distributed Learning, licensed under the Apache License, Version 2.0 (`LICENSE` here).

Taken, unchanged, from `software_development/`:

| Here | There |
|---|---|
| `packages/<id>/imsmanifest.xml` | `TestSuite/LMSRTE/Courses/LMSTestPackage_<id>/imsmanifest.xml` |
| `cases/<id>.properties` | `TestSuite/LMSRTE/Courses/TestCases/<id>.properties` |
| `Commands.properties`, `UserInterface.properties` | `resources/org/adl/testsuite/rte/lms/util/resources/` |

Not taken: the suite's lesson pages and the Java applet that drives them (it needs a Java plug-in no current browser runs), and every file a manifest names besides the manifest. The browser checks serve a replay lesson in place of each lesson page and carry out each case's commands, checks, user interface questions and navigation in the suite's order; see `../../adlLmsTest.ts`.

Added here: `descriptions.json`, what Alexandria's importer makes of each manifest, kept current by `crates/alexandria-studio/tests/scorm2004_conformance_packages.rs`.
