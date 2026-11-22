# Language Lab

Practice reading, writing, listening and speaking English. Each Language
Lab element is one activity:

| Activity | What you do | How it is scored |
|---|---|---|
| **Dictation** (श्रुतलेख) | Listen to each sentence (normal or slow) and type what you hear. | Word by word, by the built-in grader. After you submit, a colour-coded diff shows correct, missing and extra words. |
| **Reading** (पढ़ना) | Read a short passage, tap a highlighted word to see its Hindi meaning, listen to it read aloud, then answer the questions. | Correct answers ÷ questions. |
| **Writing** (लिखना) | Write English sentences for Hindi prompts. Gentle tips appear as you type (capital letter, full stop, *a* or *an*). | Each answer is checked for a capital letter, end punctuation, *a*/*an*, required words and length. An exact model answer scores full marks. |
| **Sentence builder** (वाक्य बनाइए) | Tap word tiles in the right order to build the sentence. Tap a placed word to remove it. | Correct sentences ÷ total. |
| **Read aloud** (ज़ोर से पढ़िए) | Listen to a model sentence, record yourself (up to 30 seconds), and play the two side by side. | Not scored automatically. Send a recording to your instructor for feedback, or run the on-device pronunciation check. |

The element is complete when you submit a scored activity, or when every
read-aloud sentence has a recording.

## Hindi and English

Instructions show Hindi first with English beneath. Switch to English only
with the button at the top of the activity, or with the **Instruction
language** setting. **Show Hindi hints** turns the Hindi meanings, glossary
and tips on or off. **Listening speed** sets how fast sentences are read
aloud.

## Pronunciation check

**Check pronunciation** sends your recording to a speech model that runs on
this device. It shows which words came through clearly, which were unclear
and which were missed, plus what the model heard. It checks how clearly you
were understood, not your accent: clear English with an Indian accent is
good English.

The check needs an English language pack. If it is missing, the activity
offers a button to open this plugin's settings, where **Pronunciation
language packs** downloads it. Audio never leaves the device for this check.

## Send to instructor

**Send to instructor** puts the recording in the instructor inbox (Settings
→ Plugins → Instructor Inbox) with the sentence you read. Your instructor
can listen and reply with a score and feedback. Replies appear under **My
Submissions**.

## Permissions

- **Microphone**: asked the first time you record.
- **On-device ML**: asked the first time you run the pronunciation check.

Read-aloud voices use your device's built-in text-to-speech. If no English
voice is installed, the device's default voice is used; if there is none,
the listen buttons are disabled.

## For course authors

`content_inline` is JSON with an `activity` key. Answer keys go in a
top-level `grader_private` block, which the host removes before the plugin
UI loads; only the grader sees it:

```json
{"activity":"dictation","items":[{"text":"Good morning.","hint_hi":"सुप्रभात"}]}

{"activity":"reading","passage":"My name is Priya. I live in Pune.",
 "glossary":{"live":"रहना"},
 "questions":[{"prompt":"Where does Priya live?","prompt_hi":"प्रिया कहाँ रहती है?","options":["Delhi","Pune"]}],
 "grader_private":{"answers":[1]}}

{"activity":"writing","items":[{"prompt_hi":"मेरे पास एक छाता है।","prompt_en":"I have an umbrella.","hint_hi":"umbrella से पहले an"}],
 "grader_private":{"items":[{"accept":["I have an umbrella."],"require":["umbrella"],"min_words":4}]}}

{"activity":"builder","items":[{"tiles":["rice.","I","eat"],"hint_hi":"मैं चावल खाता हूँ।"}],
 "grader_private":{"items":[{"answer":"I eat rice.","also":[]}]}}

{"activity":"read_aloud","lang":"en","items":[{"text":"Very well, thank you.","hint_hi":"v: ऊपर के दाँत नीचे के होंठ पर"}],
 "skills":["pronunciation","fluency"]}
```

Writing items may list `checks` to choose from `capital`, `end_punctuation`
and `articles` (all three by default). See the
[grader README](../language-lab-grader/README.md) for the scoring rules.
