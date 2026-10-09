'use client';

import { useState } from 'react';
import { useLocale } from 'next-intl';
import countryFlags from '@/lib/country-flags.json';

const available = new Set(countryFlags);

// Only names present in the pinned, self-hosted asset manifest become paths.
// Country text remains visible when a code or its image is unavailable.
export function CountryFlag({ country }: { country: string }) {
  const locale = useLocale();
  const code = /^[A-Za-z]{2}$/.test(country) ? country.toUpperCase() : country;
  const known = available.has(code);
  const [failedCode, setFailedCode] = useState<string | null>(null);
  let name = code;
  if (known) {
    try {
      name = new Intl.DisplayNames([locale], { type: 'region' }).of(code) ?? code;
    } catch {
      // Keep the ordinary country code on browsers without region names.
    }
  }
  return (
    <span className="country-flag" title={name} data-country={code}>
      {known && failedCode !== code && (
        // These original Twemoji SVG bytes are vendored with CC BY 4.0 attribution.
        // eslint-disable-next-line @next/next/no-img-element
        <img
          className="country-flag-image"
          src={`/flags/twemoji/${code}.svg`}
          alt={name}
          width={24}
          height={18}
          draggable={false}
          onError={() => setFailedCode(code)}
        />
      )}
      <span>{code}</span>
    </span>
  );
}
