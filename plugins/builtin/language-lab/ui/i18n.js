// Bilingual UI copy. Every entry has Hindi (`hi`) and English (`en`). With
// the Hindi setting the plugin shows Hindi first with English beneath; with
// English it shows English only. Entries may be functions of a params object.

export const STRINGS = {
  // Shared
  listen: { hi: 'सुनिए', en: 'Listen' },
  listen_slow: { hi: 'धीरे सुनिए', en: 'Slow' },
  stop_audio: { hi: 'रोकें', en: 'Stop' },
  submit: { hi: 'जमा करें', en: 'Submit' },
  submitting: { hi: 'जमा हो रहा है…', en: 'Submitting…' },
  try_again: { hi: 'फिर से कोशिश करें', en: 'Try again' },
  score: {
    hi: (p) => `आपका स्कोर: ${p.pct}%`,
    en: (p) => `Your score: ${p.pct}%`,
  },
  submitted_no_score: { hi: 'जमा हो गया।', en: 'Submitted.' },
  submit_failed: {
    hi: (p) => `जमा नहीं हो सका: ${p.error}`,
    en: (p) => `Could not submit: ${p.error}`,
  },
  need_answer: { hi: 'पहले कम से कम एक जवाब दीजिए।', en: 'Answer at least one item first.' },
  hint: { hi: 'संकेत', en: 'Hint' },
  item_n: { hi: (p) => `वाक्य ${p.n}`, en: (p) => `Sentence ${p.n}` },
  question_n: { hi: (p) => `प्रश्न ${p.n}`, en: (p) => `Question ${p.n}` },
  tts_unavailable: {
    hi: 'इस डिवाइस पर आवाज़ में पढ़कर सुनाने की सुविधा उपलब्ध नहीं है।',
    en: 'Read-aloud voice is not available on this device.',
  },
  no_english_voice: {
    hi: 'अंग्रेज़ी आवाज़ नहीं मिली, डिवाइस की डिफ़ॉल्ट आवाज़ इस्तेमाल हो रही है।',
    en: 'No English voice found, using the device default voice.',
  },
  switch_language: { hi: 'English में देखें', en: 'हिंदी में देखें' },
  unknown_activity: {
    hi: 'यह अभ्यास प्रकार पहचाना नहीं गया।',
    en: 'This activity type is not recognised.',
  },

  // Dictation
  dictation_title: { hi: 'श्रुतलेख: सुनिए और लिखिए', en: 'Dictation: listen and write' },
  dictation_intro: {
    hi: 'हर वाक्य सुनिए और जैसा सुना, वैसा अंग्रेज़ी में लिखिए। ज़रूरत हो तो धीरे सुनिए।',
    en: 'Listen to each sentence and type exactly what you hear. Use slow mode if you need it.',
  },
  type_here: { hi: 'यहाँ लिखिए…', en: 'Type here…' },
  diff_legend: {
    hi: 'हरा = सही, लाल = छूटा हुआ शब्द, नारंगी = अतिरिक्त या गलत शब्द',
    en: 'Green = correct, red = missing word, orange = extra or wrong word',
  },
  correct_sentence: { hi: 'सही वाक्य', en: 'Correct sentence' },

  // Reading
  reading_title: { hi: 'पढ़िए और जवाब दीजिए', en: 'Read and answer' },
  reading_intro: {
    hi: 'रंगीन शब्द पर टैप करके उसका हिंदी अर्थ देखिए। फिर प्रश्नों के जवाब चुनिए।',
    en: 'Tap a highlighted word to see its Hindi meaning. Then answer the questions.',
  },
  read_passage: { hi: 'पूरा पाठ सुनिए', en: 'Listen to the passage' },
  meaning: { hi: 'अर्थ', en: 'Meaning' },

  // Writing
  writing_title: { hi: 'अंग्रेज़ी में लिखिए', en: 'Write in English' },
  writing_intro: {
    hi: 'हर वाक्य अंग्रेज़ी में लिखिए। लिखते समय छोटे सुझाव दिखेंगे, इनसे स्कोर नहीं कटता।',
    en: 'Write each sentence in English. Small tips appear as you type; they do not affect your score.',
  },
  tip_capital: {
    hi: 'वाक्य बड़े अक्षर (capital letter) से शुरू कीजिए।',
    en: 'Start the sentence with a capital letter.',
  },
  tip_end_punctuation: {
    hi: 'वाक्य के अंत में पूर्ण विराम (.), प्रश्नचिह्न (?) या (!) लगाइए।',
    en: 'End the sentence with a full stop (.), a question mark (?) or (!).',
  },
  tip_articles: {
    hi: (p) => `"${p.next}" से पहले "${p.expected}" लिखिए, "${p.article}" नहीं।`,
    en: (p) => `Use "${p.expected}" before "${p.next}", not "${p.article}".`,
  },
  looks_good: { hi: 'अच्छा लग रहा है!', en: 'Looks good!' },

  // Builder
  builder_title: { hi: 'वाक्य बनाइए', en: 'Build the sentence' },
  builder_intro: {
    hi: 'शब्दों पर सही क्रम में टैप कीजिए। अंग्रेज़ी में क्रम होता है: कर्ता + क्रिया + कर्म। हटाने के लिए शब्द पर फिर से टैप कीजिए।',
    en: 'Tap the words in the right order. English order is subject + verb + object. Tap a word again to remove it.',
  },
  builder_empty: { hi: 'यहाँ आपका वाक्य बनेगा', en: 'Your sentence appears here' },
  reset: { hi: 'फिर से शुरू', en: 'Reset' },

  // Read aloud
  read_aloud_title: { hi: 'ज़ोर से पढ़िए', en: 'Read aloud' },
  read_aloud_intro: {
    hi: 'पहले वाक्य सुनिए, फिर अपनी आवाज़ रिकॉर्ड कीजिए (अधिकतम 30 सेकंड) और दोनों की तुलना कीजिए।',
    en: 'Listen first, then record yourself (up to 30 seconds) and compare the two.',
  },
  record: { hi: 'रिकॉर्ड करें', en: 'Record' },
  stop_recording: { hi: 'रिकॉर्डिंग रोकें', en: 'Stop recording' },
  record_again: { hi: 'फिर से रिकॉर्ड करें', en: 'Record again' },
  recording: {
    hi: (p) => `रिकॉर्ड हो रहा है… ${p.s} सेकंड`,
    en: (p) => `Recording… ${p.s}s`,
  },
  model_audio: { hi: 'नमूना आवाज़', en: 'Model voice' },
  your_recording: { hi: 'आपकी रिकॉर्डिंग', en: 'Your recording' },
  recorded_earlier: {
    hi: 'यह वाक्य पहले रिकॉर्ड हो चुका है। सुनने के लिए फिर से रिकॉर्ड कीजिए।',
    en: 'You recorded this earlier. Record again to play it back.',
  },
  mic_reason: {
    hi: 'आपकी आवाज़ रिकॉर्ड करने के लिए, ताकि आप उसे नमूने से मिला सकें।',
    en: 'To record your voice so you can compare it with the model.',
  },
  mic_denied: {
    hi: 'माइक्रोफ़ोन की अनुमति नहीं मिली। रिकॉर्ड करने के लिए अनुमति दीजिए।',
    en: 'Microphone permission was not given. Allow it to record.',
  },
  mic_error: {
    hi: (p) => `माइक्रोफ़ोन शुरू नहीं हो सका: ${p.error}`,
    en: (p) => `Could not start the microphone: ${p.error}`,
  },
  recorder_unavailable: {
    hi: 'इस डिवाइस पर रिकॉर्डिंग उपलब्ध नहीं है।',
    en: 'Recording is not available on this device.',
  },
  check_pronunciation: { hi: 'उच्चारण जाँचें', en: 'Check pronunciation' },
  checking: { hi: 'जाँच हो रही है…', en: 'Checking…' },
  ml_reason: {
    hi: 'आपकी रिकॉर्डिंग को इसी डिवाइस पर एक भाषा-मॉडल से जाँचने के लिए। आवाज़ कहीं भेजी नहीं जाती।',
    en: 'To check your recording with a speech model on this device. Audio is not sent anywhere.',
  },
  ml_denied: {
    hi: 'उच्चारण जाँचने की अनुमति नहीं मिली।',
    en: 'Permission for the pronunciation check was not given.',
  },
  pack_missing: {
    hi: 'उच्चारण जाँचने के लिए पहले अंग्रेज़ी भाषा-पैक डाउनलोड कीजिए।',
    en: 'Download the English language pack first to check pronunciation.',
  },
  open_settings: { hi: 'सेटिंग्स खोलें', en: 'Open settings' },
  check_unavailable: {
    hi: 'इस डिवाइस पर उच्चारण जाँच अभी उपलब्ध नहीं है।',
    en: 'Pronunciation checking is not available on this device yet.',
  },
  check_failed: {
    hi: (p) => `उच्चारण जाँच नहीं हो सकी: ${p.error}`,
    en: (p) => `The pronunciation check failed: ${p.error}`,
  },
  disclaimer: {
    hi: 'यह एक स्वचालित मॉडल की जाँच है कि आपके शब्द कितने साफ़ समझ आए। यह आपके लहजे (accent) का मूल्यांकन नहीं है — भारतीय लहजे में बोली गई साफ़ अंग्रेज़ी बिल्कुल सही है।',
    en: 'This is an automatic check of how clearly your words came through. It does not judge your accent: clear English in an Indian accent is perfectly good English.',
  },
  heard: { hi: 'मॉडल ने सुना', en: 'The model heard' },
  clarity: {
    hi: (p) => `स्पष्टता: ${p.pct}%`,
    en: (p) => `Clarity: ${p.pct}%`,
  },
  status_clear: { hi: 'साफ़', en: 'Clear' },
  status_unclear: { hi: 'अस्पष्ट', en: 'Unclear' },
  status_missed: { hi: 'छूट गया', en: 'Missed' },
  send_instructor: { hi: 'शिक्षक को भेजें', en: 'Send to instructor' },
  sending: { hi: 'भेजा जा रहा है…', en: 'Sending…' },
  sent: { hi: 'शिक्षक को भेज दिया गया', en: 'Sent to your instructor' },
  send_failed: {
    hi: (p) => `भेजा नहीं जा सका: ${p.error}`,
    en: (p) => `Could not send: ${p.error}`,
  },
  all_recorded: {
    hi: 'बहुत बढ़िया! आपने सभी वाक्य रिकॉर्ड कर लिए।',
    en: 'Well done! You have recorded every sentence.',
  },
};

/** Resolve one language of a string entry. */
export function str(key, lang, params) {
  const entry = STRINGS[key];
  if (!entry) return key;
  const v = entry[lang] ?? entry.en;
  return typeof v === 'function' ? v(params || {}) : v;
}
