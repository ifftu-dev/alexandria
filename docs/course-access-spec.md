# Course access: private courses, course staff and sharing

Status: draft
Updated: 2026-11-20
Triage: ready-for-agent
Repos: alexandria, alexandria-cloud

Everything published in Alexandria today is public, has one author, and is served mostly by that author's device. This spec adds private courses for organisations, a staff of co-instructors and teaching assistants on any course, and sharing of public courses by every device that holds them, within limits each learner sets. Background: [enterprise-boundary.md](enterprise-boundary.md), [settings.md](settings.md), [protocol-specification.md](protocol-specification.md), [learning-standards-spec.md](learning-standards-spec.md), [ADR 0004](adr/0004-tauri-free-runtime-crate-boundary.md). Vocabulary: [GLOSSARY.md](../GLOSSARY.md).

## Problem Statement

- **Publishing means publishing to everyone.** A published course is announced on the public catalog, and a node serves any file to any peer that knows its hash. There is no way to give a course to some people and not others.
- **An organisation's own material has nowhere to go.** Internal training, licensed content and anything an employer may not redistribute cannot be put into Alexandria without making it public.
- **A course has one author and one signature.** Nobody else can edit it, invite learners to it or review learner work. Teaching with a colleague or an assistant is not possible.
- **A course is only as available as its author's device.** Learners' devices fetch content and do not serve it. A course whose author is offline is hard to get.
- **The obvious fix has a cost.** Making every device serve what it holds spends learners' data and shows the network which devices hold which course. Done without limits or care, it harms the people it is meant to help.
- **Invite-only spaces nobody answers for are a risk.** An encrypted space that anyone can open and nobody can see into invites abuse, and a minor can be invited into one.

## Solution

- **A course is a draft, private or public.** Every course starts as a draft on its author's device. An individual can publish it as public. An author acting for an organisation can publish it as private or public.
- **Private courses belong to an organisation that proves its domain.** The organisation's name comes from the record on its own domain, checked by each invitee's app. Any domain owner qualifies, at no cost and with no hosted service.
- **A private course is invisible and unreadable to anyone not invited.** It never appears in the catalog. Its files are encrypted with a key only invitees hold. Members fetch it from each other and from nobody else.
- **Invites are addressed to a person.** An invite works only for the person it names and covers all their devices. The key is released only when they accept.
- **A private course can be made public once, by its owner, and never made private again.** Credentials earned before the change keep pointing at the same course.
- **A course has staff.** The owner can appoint co-instructors, who edit, publish and invite, and teaching assistants, who review learner work and moderate discussion.
- **A private course has its own discussion**, open to its members and to nobody else.
- **Every device that holds a public course helps serve it.** There is no sharing switch. Each learner sets how much data each of their devices may spend on it, on Wi-Fi and on mobile data, by day and by month.
- **Children's profiles never serve anything**, and a child needs a guardian's approval to join a private course.

## User Stories

### Learner taking public courses

1. As a learner, I want a course to stay available when its author is offline, so that I can start or continue it at any time.
2. As a learner, I want my device to help serve the public courses I hold without my having to turn anything on, so that courses I value stay reachable for people like me.
3. As a learner, I want to set how much data my device may spend on sharing over Wi-Fi and wired connections, per day and per month, so that sharing fits my line.
4. As a learner, I want to set a separate daily and monthly amount for mobile data, so that sharing never eats the plan I pay for.
5. As a learner, I want sensible starting limits that I do not have to think about, so that the app behaves well before I open settings.
6. As a learner, I want the app to tell me what limits are healthy and why sharing matters to other learners, so that I can choose with the facts in front of me.
7. As a learner, I want to be asked to confirm when I set a limit below the recommendation or to zero, and not to be reminded again after that, so that my choice is respected.
8. As a learner, I want sharing to stop for the day when the daily amount is used and for the month when the monthly amount is used, so that neither number is ever exceeded.
9. As a learner, I want any connection my device marks as metered to count as mobile data, so that a hotspot is not treated as a home line.
10. As a learner, I want sharing over mobile data to stop while my phone's data-saving mode is on, so that the phone's own setting is honoured.
11. As a learner, I want sharing to pause during a live call or an assessment, so that it never degrades what I am doing.
12. As a learner, I want sharing to pause while battery saver is on, so that it does not drain a low battery.
13. As a learner, I want to see how much each of my devices has shared today and this month, so that I know what sharing costs me.
14. As a learner, I want to see and change the limits of any of my paired devices from the device in my hand, so that I can fix a phone that is using too much without fetching it.
15. As a learner, I want the app never to delete a course I downloaded in order to save space, so that what I kept for offline use is still there.
16. As a learner, I want a file I opened straight from disk never to leave my device, so that something handed to me privately is not redistributed.
17. As a learner, I want what my device announces about its holdings not to be tied to my name or profile, so that a stranger cannot read my course list.
18. As a learner on a desktop, I want sharing to stop when I quit the app, so that nothing runs after I believe it has stopped.

### Invitee and member of a private course

19. As an invitee, I want an invite to say which organisation is behind the course, with the name read from that organisation's own domain, so that I know who is asking.
20. As an invitee, I want my app to refuse an invite whose organisation cannot be proven, so that nobody can pose as an employer or a school.
21. As an invitee, I want to see the course title and who invited me before I accept, so that I know what I am joining.
22. As an invitee, I want to be told before I accept that other members will be able to see that I am a member, so that I can decline a group I do not want to be seen in.
23. As an invitee, I want to decline an invite and have nothing about the course left on my device, so that declining is complete.
24. As an invitee, I want an invite to arrive by any channel, including a pasted link or a code I scan, so that the organisation can reach me however it already does.
25. As a member, I want a private course to work like any other course once I am in, so that I do not learn a second way of studying.
26. As a member, I want the course on every device I have paired, so that I can study on my phone after accepting on my laptop.
27. As a member, I want to fetch the course from other members when the instructor is offline, so that a small group still works.
28. As a member, I want new versions to reach me without asking, so that I always study the current course.
29. As a member, I want my device to serve the course only to other current members, so that I never hand it to an outsider by accident.
30. As a member, I want to post in the course's discussion without holding any credential, so that I can ask a question about the course I was invited to.
31. As a member, I want what I wrote in a private discussion to stay among the members if the course is later made public, so that the author cannot publish my words.
32. As a member, I want to leave a private course, block its sender and delete my copy, so that I can get out of a course I should not be in.
33. As a member, I want to be told plainly that nobody outside the course can see or moderate what happens in it, so that I know what protection I do and do not have.
34. As a member, I want a credential I earn in a private course to be checkable by an employer who cannot see the course, so that private study still counts.
35. As a member, I want my learning records from a private course to leave out its name unless the course's owner allowed the name to be disclosed, so that sending my records does not reveal a course meant to be invisible.
36. As a member, I want to be told when a private course is frozen because its organisation can no longer be proven, so that I understand why updates stopped.
37. As a removed member, I want to be told that I was removed and that I will get no further updates, so that I am not left guessing.

### Supervised learner and guardian

38. As a guardian, I want a private-course invite to my child to wait for my approval, so that nobody can bring a minor into an invisible space without my knowing.
39. As a guardian, I want to see the course title, the organisation and who sent the invite before I approve, so that I can judge it.
40. As a guardian, I want my child's device never to announce or serve course content, so that it does not advertise what my child studies or accept connections from strangers.
41. As a supervised learner, I want to see that an invite is waiting for my guardian, so that I know why I cannot open the course yet.

### Individual author

42. As an author, I want a new course to stay on my device until I publish it, so that nothing unfinished is seen.
43. As an author, I want to be told that publishing as an individual means publishing to everyone, so that I do not expect privacy I cannot have.
44. As an author, I want to appoint a co-instructor who can edit and publish my course, so that I can teach it with a colleague.
45. As an author, I want to appoint teaching assistants who can review learner work and moderate discussion but cannot change the course, so that I can share the workload safely.
46. As an author, I want to remove a staff member and have their later changes refused by every reader, so that removal takes effect.
47. As an author, I want to remain the only person who can appoint co-instructors, so that control of my course cannot pass around without me.
48. As an author, I want a warning when my own sharing limit is stopping my course from reaching learners, so that I can raise it or keep another device online.
49. As an author, I want learners' devices to carry my public course, so that I do not have to keep a machine running.

### Organisation

50. As an organisation administrator, I want to prove our organisation by a record on our own domain, with our own key and no hosted service, so that adopting private courses does not require buying anything.
51. As an organisation administrator, I want to name the people who may author private courses for us, so that our name cannot be used by someone we did not choose.
52. As an organisation administrator, I want that permission to expire and to be withdrawable, so that a person who leaves stops acting for us.
53. As an organisation administrator, I want to name a new owner for a course when its instructor leaves, so that our course does not die with one person's account.
54. As an organisation administrator, I want our signing key never to touch course content, so that holding the key is not the same as holding every course.
55. As an organisation administrator using the hosted service, I want to grant and withdraw authoring permission from the console, so that I do not handle keys myself.
56. As an organisation administrator, I want the hosted service to hold no private-course key and no private-course content, so that our material is not in a third party's hands.
57. As an organisation administrator, I want licensed material we may not redistribute to stay among the people we invite, so that we meet our licence terms.
58. As an organisation administrator, I want our course to freeze, not vanish, if our domain record lapses by mistake, so that fixing the record restores it.

### Owner of a private course

59. As an owner, I want to publish a course as private, so that only the people I invite can see it.
60. As an owner, I want my private course to be absent from the catalog and unknown to anyone not invited, so that its existence is not public.
61. As an owner, I want to invite a person by their identity, so that the invite cannot be used by whoever it is forwarded to.
62. As an owner, I want the key withheld until the invitee accepts, so that a declined invite gives nothing away.
63. As an owner, I want to see who has accepted, who has not and who has been removed, so that I know my audience.
64. As an owner, I want to remove a member and have them receive no further versions, so that someone who leaves the organisation stops getting our material.
65. As an owner, I want to be told plainly that removal cannot take back what a member already has, and that any member can copy what they can read, so that I do not rely on a guarantee that does not exist.
66. As an owner, I want to decide whether the course's title may be disclosed outside it, so that I control what a learner's credentials and records reveal.
67. As an owner, I want to make a private course public in one deliberate step, with a warning that it cannot be undone, so that I never do it by accident.
68. As an owner, I want making a course public to keep every credential already earned valid, so that early learners lose nothing.
69. As an owner, I want members' devices to start serving the course publicly without downloading it again, so that going public is quick.

### Co-instructor and teaching assistant

70. As a co-instructor, I want to edit the course and publish a new version under my own signature, so that I do not wait for the owner's device.
71. As a co-instructor, I want to invite and remove learners in a private course, so that I can manage my own cohort.
72. As a co-instructor, I want to appoint teaching assistants, so that I can get help with my section.
73. As a co-instructor, I want to endorse completions only when the course's completion rule names me, so that my authority is the one learners and verifiers can see.
74. As a teaching assistant, I want to see and review the work learners submit, so that I can give feedback.
75. As a teaching assistant, I want to moderate the course's discussion, so that I can keep it useful.
76. As a teaching assistant, I want to be unable to change the course or its audience, so that my role is clear to everyone.
77. As any staff member, I want my role shown to learners, so that they know who they are talking to.

### Verifier

78. As a verifier, I want to check a credential from a private course using a short signed summary the learner gives me, so that I need neither the course nor any service.
79. As a verifier, I want that summary to tell me who may endorse completion and which organisation stands behind the course, so that I can judge the credential.
80. As a verifier, I want the summary to carry the course's title only when its owner allowed that, so that I see no more than was meant to be shown.

### Maintainer

81. As a maintainer, I want privacy to rest on encryption and not on every device checking a list correctly, so that one faulty device cannot leak a course.
82. As a maintainer, I want readable private content never to sit in the store a node serves from, so that knowing a hash is never enough to fetch it.
83. As a maintainer, I want private-course files kept off the public record of which device holds what, so that outsiders cannot map a course's membership.
84. As a maintainer, I want the organisation rule checked by the receiving app, so that it cannot be bypassed by changing the sender's app.
85. As a maintainer, I want every signed format in this spec defined once in the permissively licensed verification crate, so that the app, the hosted service and independent verifiers agree byte for byte.
86. As a maintainer, I want the decision to serve or not to be a pure function of limits, usage, connection and device state, so that it can be tested without a network.
87. As a maintainer, I want a course's identity to stay the same across owners, staff changes and the move from private to public, so that credentials and learning records never dangle.
88. As a maintainer, I want it established that a device's content address is not tied to a named profile before sharing ships, so that the privacy condition is checked and not assumed.
89. As a maintainer, I want live tutoring and classrooms left as they are and not called private, so that the app does not claim one level of protection for surfaces that differ.
90. As a maintainer, I want each phase shippable alone, so that sharing, staff and private courses do not wait on each other.

## Implementation Decisions

### Vocabulary

New terms, to be added to the glossary when the work starts.

- **Course visibility**: one of draft, private or public. Applies to courses and tutorials, which are the same signed document.
- **Private course**: a course that only its invited members can find or read.
- **Course member**: a person who accepted an invite to a private course and has not been removed.
- **Course invite**: a signed message offering one named person membership of a private course.
- **Course key**: the secret a private course's content is encrypted with. A course has one key generation at a time and keeps earlier ones.
- **Authoring delegation**: an organisation's signed statement that a named person may publish private courses under its domain.
- **Course owner**: the one person who holds a course's irreversible powers. **Co-instructor** and **teaching assistant** are the other two staff roles.
- **Staff list**: the owner-signed list of a course's staff and their roles.
- **Course summary**: a short signed statement of a course's identity and completion rule, given to a verifier who cannot see the course.
- **Course discussion**: a discussion that belongs to one course.
- **Sharing limit**: the amount of data a device may spend serving content to others, per connection class, per day and per month.

### Visibility

- Every course starts as a draft. A draft exists only on its author's devices and is neither announced nor served.
- An individual author's only way to publish is as public.
- An author holding a valid authoring delegation chooses private or public when publishing, with private preselected.
- Private to public is the only change of visibility after publishing. It happens once and cannot be reversed. There is no way to make a public course private, because copies already exist on other people's devices.
- Visibility is not a field of the signed course document. A document's hash is therefore the same while private and after it is made public.
- Classrooms and live tutoring are unchanged. Anyone can still create them. They are not described as private anywhere in the app; see Further Notes for why.

### Organisation authoring

- A private course needs an organisation that publishes a domain descriptor, the existing record on its own domain that lists the keys allowed to sign for it.
- This is an accountability rule and not a commercial one. Any domain owner qualifies. No account with the hosted service is involved.
- **The check is on the receiving side.** An invitee's app looks up the descriptor itself, as it already does for an access request, and refuses an invite whose chain does not check out. A sender's app that skipped the rule would produce invites nobody's app accepts.
- **The organisation's key signs a delegation, not a course.** An authoring delegation names a person's DID, the organisation's domain and an expiry. The person then publishes private courses with their own key. The organisation's key never encrypts, signs or holds course content.
- Delegations expire. The starting lifetime is 90 days, renewable. An organisation withdraws one early by signing a withdrawal, which any member's app passes on over the course's private channel once it has seen it. Removing a key from the descriptor withdraws everything that key signed, as it does for credentials.
- **Self-serve tooling.** The developer CLI can create an organisation key, print the record to publish and sign a delegation or a withdrawal, so that an organisation needs nothing but its domain. The in-app equivalent can follow.
- **When the proof stops checking out** (the record is gone, the key is no longer listed, the delegation has expired or been withdrawn) the course freezes: members keep and can use what they have, and no invite, acceptance, new version or move to public is accepted until the proof checks out again. The chain is checked at invite, at acceptance and at each new version. A version that arrives while the domain cannot be looked up is held and not applied until it can be.
- **The organisation owns its private courses.** It can sign an ownership statement naming a new owner for a course id. A new owner who lacks the course key receives it from any member's device, which releases it on seeing a valid ownership statement. The organisation learns a course id only when someone tells it; nothing reports courses to it.
- An individual's public course has no such recovery. If its owner's identity is lost for good, the course stays readable and can no longer change.

### Course identity and document format

- A course's identity is fixed when it is created and records the identity of whoever created it. It does not change when the owner changes, when staff change or when the course is made public.
- A course version's identity is the hash of its readable signed document. Completion endorsements and credentials already bind to that hash, so they are unaffected by encryption and by the move to public.
- The signed course document changes to separate three things it treats as one today: who created the course, who signed this version, and which staff list gives the signer the right to. This is a new document format version. The project is pre-launch with disposable data, so older documents are replaced and no migration is written.
- A reader accepts a version only if its signer is the owner or a co-instructor on the newest staff list that reader has seen for the course, and, for an organisation's course, the organisation's chain checks out.
- These acceptance rules, and every signed format in this spec, are defined in the permissively licensed verification crate with no I/O, so that the app, the hosted service and independent verifiers apply the same rules to the same bytes.

### Course staff

Roles apply to public and private courses alike.

| Power | Owner | Co-instructor | Teaching assistant |
|---|---|---|---|
| Edit and publish versions | yes | yes | no |
| Invite and remove learners (private courses) | yes | yes | no |
| Appoint and remove teaching assistants | yes | yes | no |
| Appoint and remove co-instructors | yes | no | no |
| Make a private course public | yes | no | no |
| Decide whether the title may be disclosed | yes | no | no |
| Review submissions and moderate discussion | yes | yes | yes |

- There is exactly one owner. For an individual's course it is the creator. For an organisation's course it is whoever the newest ownership statement names, and the creator if there is none.
- The staff list is signed by the owner, carries a version number that only rises, and travels with the course: with the catalog announcement for a public course, over the private channel for a private one. Teaching assistants appointed by a co-instructor are added by a signed entry from that co-instructor, which the staff list's rules allow.
- Removing a staff member is a new staff list. A version signed by someone the newest list no longer names is refused.
- **Endorsing completion is not a staff power.** Endorsers remain the explicit list in a course version's completion rule. Appointing a co-instructor does not make them an endorser until a published version names them. This keeps the completion rule fixed for each version, which is what lets a credential be checked later.
- Learner submissions that go to a course's author today go to everyone with the review power.
- On a private course, staff are members. They receive the course key through the same invite flow, with their role attached.

### Private course protection

- **The promise.** People who were not invited cannot read a private course and cannot learn that it exists. Nothing is promised against a member copying or passing on what they can read, and the app says so to owners.
- **Encryption is the guarantee.** The course document and every file are encrypted with the course key. Whoever serves the bytes cannot read them and need not be trusted.
- Files are encrypted in chunks so that a range can be read without the whole file, which video seeking and partial transfer both need.
- Encryption is deterministic for a given key generation and file, so every member derives the same encrypted bytes and the same address for them. A member can therefore serve a file it decrypted, and no central list of addresses is needed.
- **Readable copies stay out of the served store.** While a course is private, its readable bytes exist only in memory or in the profile's local cache. The store a node serves from holds only the encrypted form, kept apart from public content.
- **Members-only serving.** Encrypted private-course files are served over a separate members-only path that checks a membership proof before sending any bytes. They are never served by the public path that answers any request for a known hash. This is a second layer, so that outsiders cannot even collect the encrypted bytes.
- **A private channel per course.** Members find each other, receive versions, receive membership changes and exchange discussion posts over a channel whose name is derived from a course secret and whose messages are encrypted. Per-classroom channels are the prior art.
- **Nothing on public topics.** A private course is never sent to the catalog, and its files are never announced on the public record of which device holds what.
- **Size.** The design targets audiences of up to a few hundred members. The course key is sealed once per member, and membership changes are sent to every member.

### Invites and membership

- An invite is signed by a staff member with the invite power, names one person's DID and a role, and carries the chain a recipient needs to check: the staff list entry, the authoring delegation and the organisation's domain.
- **An invite names its organisation in the clear and seals the rest.** The course title, description and inviter are sealed to the invitee, so a forwarded invite reveals only that some organisation invited someone to something.
- An invite can travel by any channel: sent directly between devices, pasted as a link or scanned as a code. It is useless to anyone but the person it names.
- **The key is released on acceptance.** The invitee signs an acceptance. A device of any staff member with the invite power checks it, records the membership and releases every key generation sealed to the invitee. One such device must be reachable once per acceptance. There is no server mailbox in this spec.
- How a key agreement key is bound to a DID is settled in the first ticket of the private-course phase. The classroom group key exchange and device pairing are the prior art.
- Declining leaves nothing on the invitee's device.
- **Membership is a set of signed entries**: each member's invite and acceptance, and each removal signed by staff. Any member can check any entry. The entries travel over the private channel.
- **Members see each other.** The invite says so before acceptance.
- **Accepting grants access to a person.** Their other paired devices receive the membership and the key generations through the existing device sync. No device needs separate approval.
- Accepting an invite does not enrol the learner. Enrolling stays the separate step it is today.
- Only staff invite. A member cannot invite anyone.

### Versions, removal and key generations

- A new version is sent over the private channel under the current key generation. A member who was offline receives it from whichever member it next reaches.
- **Removing a member** adds a signed removal to the membership entries and starts a new key generation, sealed to every remaining member. Files added or changed from then on use the new generation. Files that did not change are not re-encrypted, since the removed member already had them.
- A new member receives every generation, because old files still use old ones.
- A removed member's app is told of the removal and stops receiving versions. The app never claims that removal deleted anything.
- A member leaving on their own deletes their copy and their keys and tells the channel, which is handled as a removal.

### Making a course public

- Only the owner can do it. The app asks for confirmation and states that it cannot be undone.
- The owner publishes the same readable signed document to the public catalog. Its hash, and the course's identity, are unchanged.
- Each member's device, on seeing the owner's signed statement, decrypts the files it holds into the public served store under their readable hashes and drops the encrypted copies. It then serves the course like any other public course. Nothing is downloaded again.
- From then on the course follows the public rules in this spec, including sharing by every holder.
- An organisation's course stays the organisation's after it is public. Readers still check the organisation's chain for new versions and show its name. If the proof lapses, existing versions stay readable and new ones are refused until it is restored.
- No further invites are issued. The membership at that moment stays as the audience of the course's private discussion.

### Course discussions

- A course discussion belongs to one course. There are no standalone private threads; a classroom channel already serves that purpose.
- **In a private course**, posts travel over the private channel, encrypted under the course key. Membership is the only condition for posting. The credential checks that gate the public forum do not apply.
- Nothing posted or voted in a private course discussion counts towards anyone's public standing.
- Staff with the moderation power can hide a post for all members by a signed action that members can see was taken.
- **When a course is made public, its existing discussion stays private.** It remains readable and writable by the people who were members at that moment and by nobody else. The public course starts a new, empty discussion.
- A public course's discussion follows the public forum's existing rules. See Further Notes for a known tension.

### Credentials and learning records

- **Course summary.** For any course version, staff who may publish it can sign a course summary: the course's identity, the version's document hash, its completion rule, the creator, the signer, and for an organisation's course the chain to its domain. A learner hands it over with a credential. A verifier checks the credential's endorsements against the completion rule in the summary and needs neither the course nor any service.
- The summary carries the course's title only if the owner has allowed the title to be disclosed. That switch is off by default for a private course.
- The summary format and its check live in the verification crate, which keeps the standing guarantee that checking a credential is free, works offline and needs no commercial service.
- **Learning records follow the same switch.** Records that leave the device from a private course carry identifiers only, and omit the course's and its activities' names, unless the owner allowed disclosure. The consent screen for sending records says the course is private. The record formats are specified in [learning-standards-spec.md](learning-standards-spec.md).
- A package element in a private course is encrypted and distributed like any other file in it. This is where an organisation's licensed or internal packages belong.

### Supervised profiles

- On a supervised profile, accepting a private-course invite waits for the guardian. The request travels over the existing sealed guardian link and shows the guardian the course title, the organisation and the inviter. The guardian approval already used for AI chat is the prior art.
- A guardian continues to see a supervised learner's enrolments and course titles, private courses included, as for any course.
- **A supervised profile never announces or serves content**, public or private. This is fixed and is not a setting.

### Sharing public courses

- **No sharing switch.** There is no global or per-course setting that turns sharing on or off. A device that holds content of a public course announces it on the existing public record and serves it over the existing public path. This covers every content type.
- A device holds a public course's content once it has fetched it for a course its user opened or enrolled in. It stops announcing a file when the user removes the download.
- Members of a private course serve each other only, as described above. The same limits apply.
- **Never shared**: a draft; a file a learner opened straight from disk outside any published course; anything on a supervised profile.
- **A condition on shipping.** An announcement carries a device's content address and nothing else. The first ticket of the sharing phase establishes whether that address is tied to a named profile in any public message the app sends. If it is, the tie is removed before sharing ships. An author is the exception by nature: publishing a course ties the author's identity to it.
- Sharing happens only while the app's process is running and a profile is unlocked. No tray icon and no start at login are added on desktop. On Android the existing background service means sharing continues after the app is put away, within the limits. On iOS it stops when the app leaves the screen.
- This spec adds no automatic deletion of downloaded content.

### Sharing limits

- Limits are set per device and per connection class. There are two classes: **Wi-Fi and wired**, and **mobile data**. A connection the system marks as metered counts as mobile data. Where the system gives no signal, the connection counts as Wi-Fi and wired.
- Each class has a daily amount and a monthly amount. Both apply and the stricter wins: sharing stops for the day at the daily amount, and until the next month at the monthly amount. A day and a month are calendar periods on the device's clock.

| Connection class | Daily | Monthly |
|---|---|---|
| Wi-Fi and wired | 3 GB | 50 GB |
| Mobile data | 50 MB | 250 MB |

- These defaults are also the recommended healthy limits shown to the learner. They are to be revisited once real usage exists; their basis is in Further Notes.
- Zero is allowed for any amount. Zero everywhere stops a device sharing.
- **One budget per device.** It covers everything the device serves to others: public courses, private-course members and courses its user authored. An author is warned when their limit is stopping their own course from being served.
- A transfer in progress stops when the budget runs out. Transfers are by verified ranges, so the receiver continues from another holder.
- **Limits and usage are visible across a person's paired devices.** Limits are synced settings keyed by device. Each device also syncs its usage totals for today and for the month. From any device a learner sees every paired device with its limits and usage and can change any device's limits; the change applies when that device next syncs. If two devices change the same limit, the later change wins.
- **The settings screen explains once** why sharing keeps courses reachable for other learners and what the recommended limits are. Setting an amount below the recommendation, or to zero, asks for confirmation. There are no recurring reminders.
- **Automatic pauses**, which are not settings: sharing stops during a live call, during an assessment and while battery saver is on, and sharing over mobile data stops while the phone's data-saving mode is on.
- Upload speed is capped at a fixed value chosen in the sharing phase from measurement on a typical home line and a typical phone. It is not a setting.
- **The decision to serve is one pure function** of the limits, the usage so far, the connection class, the device state and the clock. It returns either an amount that may be served or a reason for refusing. The app reports that reason where a learner or author would look for it.
- Whether the transfer library lets the host count and stop a transfer in progress is confirmed in the first ticket of the sharing phase; if it does not, the transfer is wrapped so that it can.

### Hosted service

- The hosted service gains one thing: when it holds an organisation's signing key, an administrator can grant, renew and withdraw authoring delegations and sign an ownership statement from the console. Each is written to the organisation's audit log.
- It never receives a course key, a private course's content, its membership or its discussion.
- Inviting an organisation's roster automatically is a later layer and not part of this spec.

### Enterprise boundary

Rows to add to the worked examples when the work lands:

| Feature | Verdict |
|---|---|
| Private courses, invites, membership, course keys | Core |
| Course staff and the staff list | Core |
| Checking the organisation behind a private course | Core |
| Creating an organisation key and signing delegations with the CLI | Core |
| Sharing public courses and the sharing limits | Core |
| The course summary and its check | Core, in the permissively licensed verification crate |
| Signing delegations with a key the hosted service holds | Enterprise |
| Inviting an organisation's roster automatically | Enterprise, later |

### Sequence

Each phase ships on its own. Phase 1 depends on nothing else here. Phase 4 needs phases 2 and 3.

1. Sharing public courses: the content-address check, transfer metering, announcing on fetch, the serve decision, limits and their settings, usage sync, automatic pauses, the supervised-profile rule.
2. Course staff: the new document format, the staff list, version acceptance, co-instructor publishing, reviewers for submissions.
3. Organisation authoring: the delegation, withdrawal and ownership formats, the CLI tooling, the console action in the hosted service.
4. Private courses: key agreement binding, encryption, the private channel, members-only serving, invites and acceptance, versions, removal and key generations, guardian approval, the freeze rule.
5. Course discussions, the course summary and the disclosure switch for learning records.
6. Making a private course public.

## Testing Decisions

A good test here drives what a person or another device can observe: an invite goes in and a readable course or a refusal comes out; a request for a file is answered or declined; a credential and a summary check or fail. Tests do not assert on message layouts, table shapes or how keys are stored.

Three seams:

1. **Two-party protocol seam (Rust, in-memory databases, real cryptography).** Two or more profiles, each with its own database and freshly generated keys, exchange the production messages as bytes, with the transport treated as a transparent carrier. This is the main seam and covers: invite, acceptance and key release; refusal of an invite whose organisation chain fails, in each way it can fail; an outsider and a removed member refused by members-only serving; a version reaching an offline member through a third member; removal and the next key generation; a co-instructor's version accepted and a removed co-instructor's refused; ownership reassignment and key release to the new owner; the freeze rule and recovery from it; guardian approval and refusal; making a course public, asserting the document hash is unchanged, members serve it publicly, and the old discussion stays closed to non-members; a private discussion post not reaching public standing. It also covers sharing: a device announces what it fetched for a public course, never announces a draft, a file opened from disk, a private-course file on the public record, or anything on a supervised profile. The serve decision is exercised here with an injected clock, connection class and device state, across daily and monthly limits, zero, the data-saving and battery pauses, and a budget running out mid-transfer. Limit and usage sync between two paired devices, including a remote change and a conflicting one, is covered here too. Prior art: the guardian link and settings sync end-to-end tests, which model two parties with real keys and drive the production handlers directly.
2. **Verification crate vectors (no I/O).** Fixed test vectors for every signed format: authoring delegation, withdrawal, ownership statement, staff list, invite, acceptance, removal, course summary, and the version acceptance rule. They include a credential from a private course checked with only its summary. The hosted service's signed output is checked against the same vectors in its existing integration tests against Postgres. Prior art: the completion endorsement vectors.
3. **Desktop interface seam (browser harness, Chromium and WebKit, fake backend).** The publish flow offers the right choices for an individual and for a delegated author; the confirmation for making a course public; the invite screen shows the organisation's name and the members-can-see-you notice before acceptance; a supervised profile shows an invite as waiting; the limits screen shows defaults, the explanation, the confirmation below the recommendation, and the list of paired devices with usage; no surface describes a classroom or a tutoring session as private. Prior art: the desktop interface checks with fixtures and a fake backend.

Checks that stay manual and are recorded with each phase: two physical devices on a real network exchanging a private course with the owner offline; an Android phone in the background obeying its mobile-data limit and the data-saving pause; an iOS device; and a metered connection being detected on each desktop platform.

## Out of Scope

- Changing who can join a live tutoring session or a classroom, or removing the classroom plaintext fallback.
- Standalone private discussion threads.
- Protection against a member copying, recording or re-publishing what they can read.
- Reporting or moderation of a private course by anyone outside it.
- Making a public course private.
- Members inviting other members.
- Shared ownership, and any rule for owners who disagree.
- Audiences larger than a few hundred, and inviting an organisation's roster automatically.
- The hosted service holding course keys, content or membership.
- A server mailbox for invites to people whose devices are offline.
- Hiding members from one another.
- Hiding a device's network address from the peers it exchanges files with.
- Running in the background on desktop.
- Automatic deletion of downloaded content, or a storage limit for sharing.
- Paid courses.

## Further Notes

- **Decisions reversed along the way.** An earlier answer had new courses start private for everyone, which would have let an individual tutor run a private course with no domain. Requiring an organisation's domain replaced it. A tutor who wants a closed group needs a domain or uses a classroom. An earlier answer also kept a sharing switch; it was removed in favour of limits, where zero has the same effect.
- **What the organisation rule buys.** A domain costs little and can be registered without naming anyone. The rule gives members a name they can see and a registrar to complain to. It is not moderation, and nobody outside a private course can read it to judge it. Guardian approval is the protection for minors.
- **What intermediaries can still see.** Peers and relays that carry a private channel see that a set of devices exchange messages on one opaque topic. They cannot read the messages or learn which course it is. Members see each other's network addresses.
- **What sharing reveals.** With every holder announcing, anyone on the network can see which device addresses hold which public course. The condition that an address is never tied to a named profile is what keeps this from being a public fact about a person, and it is checked before the sharing phase ships. A network address can still be linked to a person by whoever can map addresses to subscribers.
- **Basis of the default limits.** Figures looked up on 2026-11-20: average mobile data use per smartphone is about 22 GB a month worldwide, 37 GB in the India region and 5.3 GB in Sub-Saharan Africa (Ericsson Mobility Report, June 2026); an average home line in the United States carries about 711 GB a month in total and 58 to 94 GB of upload (OpenVault, second quarter 2026); fair-use ceilings found for home lines run from 1 TB to 3.3 TB, with a 400 GB plan the tightest found. The Wi-Fi default is 5% of a 1 TB ceiling and below an average home's existing upload. The mobile default is about 1% of average worldwide use.
- **Where the defaults still hurt.** 250 MB is about 5% of an average month in Sub-Saharan Africa, where data is least affordable; one worldwide number cannot be fair to every market. A hotspot or a home router on a capped mobile plan looks like plain Wi-Fi when the system does not mark it metered. The data-saving pause and the metered rule cover part of both.
- **Storage.** Because holders serve what they hold, a device keeps content in the form it serves. For standards packages this means keeping the packed archive; the learning standards spec treats the unpacked copy as a cache.
- **Public course discussions and the credential gate.** A public course's discussion follows the public forum's posting rules, which require a credential in the subject field. That can stop an ordinary learner asking a question about a course they are taking. This was assumed and not decided; it deserves a decision before phase 5.
- **Live tutoring and classrooms are weaker than a private course.** A tutoring session admits anyone who holds its link, and links can be forwarded. A classroom needs a moderator's approval and encrypts messages, but falls back to unencrypted messages when it has no group key, and its calls use the same forwardable links. Closing these gaps is separate work. Until then the app does not call either private.
- **Assumptions made without asking**, each open to reversal: one spec covers visibility, staff and sharing; staff on a private course get the key through the ordinary invite flow; removing staff works like removing a member plus a new staff list; self-serve organisation tooling goes in the CLI first; the 90-day delegation lifetime; the fixed upload speed cap being left to measurement.
- **Documentation to update with each phase**, proposed alongside the code: the glossary, the protocol specification for the new formats and the private channel, the architecture document, the database schema reference, the settings document for the limits, the enterprise boundary examples, the multi-user document for the supervised rules, and the hosted service's organisation document.

## Notes

- 2026-11-20: Spec written from a design interview. No code yet.
