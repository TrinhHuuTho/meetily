import React, { useState, useEffect } from "react";
import { invoke } from '@tauri-apps/api/core';
import { getVersion } from '@tauri-apps/api/app';
import Image from 'next/image';

const REPOSITORY_URL = 'https://github.com/TrinhHuuTho/trinote';
const UPSTREAM_URL = 'https://github.com/Zackriya-Solutions/meeting-minutes';

const FEATURES = [
    {
        title: 'Private by design',
        description: 'Audio, transcripts and summaries stay on your computer. No telemetry, no account.',
    },
    {
        title: 'English, Vietnamese, Chinese',
        description: 'Each speech segment is detected and transcribed in its own language, even in mixed meetings.',
    },
    {
        title: 'Local AI summaries',
        description: 'Summarize with the built-in model or Ollama, or bring your own API key.',
    },
    {
        title: 'PDF export',
        description: 'Share the summary and the timestamped transcript as one PDF.',
    },
];

export function About() {
    const [currentVersion, setCurrentVersion] = useState<string>('');

    useEffect(() => {
        getVersion().then(setCurrentVersion).catch(console.error);
    }, []);

    const openUrl = async (url: string) => {
        try {
            await invoke('open_external_url', { url });
        } catch (error) {
            console.error('Failed to open link:', error);
        }
    };

    return (
        <div className="p-4 space-y-4 h-[80vh] overflow-y-auto">
            {/* Compact Header */}
            <div className="text-center">
                <div className="mb-3">
                    <Image
                        src="icon_128x128.png"
                        alt="TriNote Logo"
                        width={64}
                        height={64}
                        className="mx-auto"
                    />
                </div>
                <h1 className="text-xl font-bold text-gray-900">TriNote</h1>
                {currentVersion && <span className="text-sm text-gray-500">v{currentVersion}</span>}
                <p className="text-medium text-gray-600 mt-1">
                    Meeting notes and summaries that never leave your machine.
                </p>
            </div>

            {/* Features Grid - Compact */}
            <div className="grid grid-cols-2 gap-2">
                {FEATURES.map(feature => (
                    <div key={feature.title} className="bg-gray-50 rounded p-3">
                        <h3 className="font-bold text-sm text-gray-900 mb-1">{feature.title}</h3>
                        <p className="text-xs text-gray-600 leading-relaxed">{feature.description}</p>
                    </div>
                ))}
            </div>

            {/* Open source attribution */}
            <div className="bg-blue-50 rounded p-3 space-y-2">
                <p className="text-sm text-blue-900">
                    TriNote is open source and based on <span className="font-semibold">Meetily</span> by
                    Zackriya Solutions, used under the MIT License.
                </p>
                <div className="flex flex-wrap gap-2">
                    <button
                        onClick={() => openUrl(REPOSITORY_URL)}
                        className="px-3 py-1.5 bg-blue-600 hover:bg-blue-700 text-white text-sm font-medium rounded transition-colors"
                    >
                        Source code
                    </button>
                    <button
                        onClick={() => openUrl(UPSTREAM_URL)}
                        className="px-3 py-1.5 bg-white hover:bg-gray-50 border border-blue-200 text-blue-700 text-sm font-medium rounded transition-colors"
                    >
                        Original Meetily project
                    </button>
                </div>
            </div>

            {/* Footer - Compact */}
            <div className="pt-2 border-t border-gray-200 text-center">
                <p className="text-xs text-gray-400">
                    Maintained by Trinh Huu Tho
                </p>
            </div>
        </div>
    )
}
