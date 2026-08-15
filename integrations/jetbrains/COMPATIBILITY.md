# CKB JetBrains compatibility

CKB publishes compatibility as two Marketplace lines:

- **1.8.x** — legacy line for IntelliJ Platform builds 233 through 243.*.
- **1.9.x** — modern line starting at build 243 (2024.3), built with Java 21 and IntelliJ Platform Gradle Plugin 2.x.

The 1.9.x release pipeline runs IntelliJ Plugin Verifier against both its 2024.3 build target and the current stable IntelliJ IDEA 2026.2 line before Marketplace publication.

The plugin declares the Java plugin dependency explicitly because CKB IDE intelligence uses Java PSI APIs. Other JetBrains products are eligible only where the required platform/Java dependency is available.

Do not widen compatibility by editing only `plugin.xml`; update the Gradle verification matrix and require a green verifier run first.
