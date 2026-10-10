import React, { useState, useEffect } from 'react';
import { Globe } from 'lucide-react';
import { toast } from 'sonner';
import { listen } from '@tauri-apps/api/event';
import { WhisperAPI } from '@/lib/whisper';
import { useConfig } from '@/contexts/ConfigContext';
import { LANGUAGES, MULTILINGUAL_EN_VI_ZH, isAutoLanguage } from '@/constants/languages';


// Small multilingual models the backend can use for fast language detection
const LANGUAGE_DETECTOR_MODELS = ['base', 'base-q5_1', 'tiny', 'tiny-q5_1', 'small-q5_1', 'small'];

/**
 * Restricted auto-detection is fast only with a small detector model downloaded;
 * otherwise the main model runs an extra full encoder pass per segment.
 */
function LanguageDetectorModelHint() {
  const [hasDetector, setHasDetector] = useState<boolean | null>(null);
  const [progress, setProgress] = useState<number | null>(null);

  useEffect(() => {
    WhisperAPI.getAvailableModels()
      .then(models =>
        setHasDetector(models.some(m => LANGUAGE_DETECTOR_MODELS.includes(m.name) && m.status === 'Available'))
      )
      .catch(() => setHasDetector(null));

    const unlisteners = [
      listen<{ modelName: string; progress: number }>('model-download-progress', event => {
        if (event.payload.modelName === 'base') setProgress(event.payload.progress);
      }),
      listen<{ modelName: string }>('model-download-complete', event => {
        if (event.payload.modelName === 'base') {
          setProgress(null);
          setHasDetector(true);
        }
      }),
    ];
    return () => {
      unlisteners.forEach(unlisten => unlisten.then(fn => fn()));
    };
  }, []);

  if (hasDetector !== false) return null;

  const download = async () => {
    setProgress(0);
    try {
      await WhisperAPI.downloadModel('base');
    } catch (error) {
      setProgress(null);
      toast.error('Failed to download the language detection model', {
        description: error instanceof Error ? error.message : String(error),
      });
    }
  };

  return (
    <div className="mt-2 flex items-center justify-between gap-2">
      <p className="text-xs">
        Download the small <strong>base</strong> model (142 MB) to keep language detection fast.
      </p>
      <button
        onClick={download}
        disabled={progress !== null}
        className="text-xs px-2 py-1 rounded bg-blue-600 text-white disabled:bg-blue-300 flex-shrink-0"
      >
        {progress === null ? 'Download' : `${progress}%`}
      </button>
    </div>
  );
}

interface LanguageSelectionProps {
  selectedLanguage: string;
  onLanguageChange: (language: string) => void;
  disabled?: boolean;
  provider?: 'localWhisper' | 'parakeet' | 'deepgram' | 'elevenLabs' | 'groq' | 'openai';
}

export function LanguageSelection({
  selectedLanguage,
  onLanguageChange,
  disabled = false,
  provider = 'localWhisper'
}: LanguageSelectionProps) {
  const [saving, setSaving] = useState(false);
  const { setSelectedLanguage } = useConfig();

  // Parakeet only supports auto-detection (doesn't support manual language selection)
  const isParakeet = provider === 'parakeet';
  const availableLanguages = isParakeet
    ? LANGUAGES.filter(lang => lang.code === 'auto' || lang.code === 'auto-translate')
    : LANGUAGES;

  const handleLanguageChange = async (languageCode: string) => {
    setSaving(true);
    try {
      // Save language preference to localStorage and sync to backend
      setSelectedLanguage(languageCode);
      onLanguageChange(languageCode);
      console.log('Language preference saved:', languageCode);

      // Show success toast
      const selectedLang = LANGUAGES.find(lang => lang.code === languageCode);
      const languageName = selectedLang?.name || languageCode;
      toast.success("Language preference saved", {
        description: `Transcription language set to ${languageName}`
      });
    } catch (error) {
      console.error('Failed to save language preference:', error);
      toast.error("Failed to save language preference", {
        description: error instanceof Error ? error.message : String(error)
      });
    } finally {
      setSaving(false);
    }
  };

  // Find the selected language name for display
  const selectedLanguageName = LANGUAGES.find(
    lang => lang.code === selectedLanguage
  )?.name || 'Auto Detect (Original Language)';

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <Globe className="h-4 w-4 text-gray-600" />
          <h4 className="text-sm font-medium text-gray-900">Transcription Language</h4>
        </div>
      </div>

      <div className="space-y-2">
        <select
          value={selectedLanguage}
          onChange={(e) => handleLanguageChange(e.target.value)}
          disabled={disabled || saving}
          className="w-full px-3 py-2 text-sm bg-white border border-gray-300 rounded-md shadow-sm focus:outline-none focus:ring-1 focus:ring-blue-500 focus:border-blue-500 disabled:bg-gray-50 disabled:text-gray-500"
        >
          {availableLanguages.map((language) => (
            <option key={language.code} value={language.code}>
              {language.name}
              {!isAutoLanguage(language.code) && ` (${language.code})`}
            </option>
          ))}
        </select>

        {/* Parakeet language limitation warning */}
        {isParakeet && (
          <div className="p-2 bg-amber-50 border border-amber-200 rounded text-amber-800">
            <p className="font-medium">ℹ️ Parakeet Language Support</p>
            <p className="mt-1 text-xs">Parakeet currently only supports automatic language detection of European languages. Use Local Whisper to pick a specific language or to transcribe Vietnamese and Chinese (including mixed English / Vietnamese / Chinese meetings).</p>
          </div>
        )}

        {/* Info text */}
        <div className="text-xs space-y-2 pt-2">
          <p className="text-gray-600">
            <strong>Current:</strong> {selectedLanguageName}
          </p>
          {selectedLanguage === 'auto' && (
            <div className="p-2 bg-yellow-50 border border-yellow-200 rounded text-yellow-800">
              <p className="font-medium">⚠️ Auto Detect may produce incorrect results</p>
              <p className="mt-1">For best accuracy, select your specific language (e.g., English, Spanish, etc.)</p>
            </div>
          )}
          {selectedLanguage === MULTILINGUAL_EN_VI_ZH && (
            <div className="p-2 bg-blue-50 border border-blue-200 rounded text-blue-800">
              <p className="font-medium">🌏 English / Vietnamese / Chinese</p>
              <p className="mt-1">
                Each speech segment is detected as English, Vietnamese or Chinese and transcribed in
                that language. Requires the Local Whisper engine (large-v3-turbo recommended).
              </p>
              {!isParakeet && <LanguageDetectorModelHint />}
            </div>
          )}
          {selectedLanguage === 'auto-translate' && (
            <div className="p-2 bg-blue-50 border border-blue-200 rounded text-blue-800">
              <p className="font-medium">🌐 Translation Mode Active</p>
              <p className="mt-1">All audio will be automatically translated to English. Best for multilingual meetings where you need English output.</p>
            </div>
          )}
          {!isAutoLanguage(selectedLanguage) && (
            <p className="text-gray-600">
              Transcription will be optimized for <strong>{selectedLanguageName}</strong>
            </p>
          )}
        </div>
      </div>
    </div>
  );
}
