# Packages and records in private courses

Triage: ready-for-agent
Repos: alexandria
Part of: docs/learning-standards-spec.md
Blocked by: docs/course-access-spec.md

Spec: Package storage and import → private course bullet; Distribution → third bullet; Sending records → "Records from a private course do not name it by default".

Cannot start until private courses exist.

- In a private course a package's archive is stored encrypted like every other file of that course. Extracted files are readable only in the profile's local cache, never in the store the node serves from.
- Learning records that leave the device from a private course carry identifiers and omit the course's and its activities' names, unless the course's owner allowed its title to be disclosed. The consent screen says the course is private.

Done when: host-seam tests show a package in a private course launches from an encrypted archive, and records sent from a private course omit names with disclosure off and carry them with it on.
