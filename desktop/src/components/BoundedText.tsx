import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { textSegments } from '../lib/textSegments';
import './BoundedText.css';

export function BoundedText({ text, className, raw }: { text: string; className?: string; raw?: boolean }) {
  const { t } = useTranslation();
  const offsets = useMemo(() => textSegments(text), [text]);
  const [selectedPage, setSelectedPage] = useState(0);
  // Streaming updates keep the current segment. Shrinking content clamps it;
  // switching records is reset by the caller's key.
  const page = Math.min(selectedPage, offsets.length - 1);
  const start = offsets[page];
  const end = offsets[page + 1] ?? text.length;
  return <div className="mux-bounded-text">
    {offsets.length > 1 && <div className="mux-text-segments" aria-label={t('textSegments.label')}>
      <span>{t('textSegments.position', { current: page + 1, total: offsets.length })}</span>
      <button type="button" disabled={page === 0} onClick={() => setSelectedPage(page - 1)}>{t('textSegments.previous')}</button>
      <button type="button" disabled={page + 1 === offsets.length} onClick={() => setSelectedPage(page + 1)}>{t('textSegments.next')}</button>
    </div>}
    <pre tabIndex={0} className={className} data-raw={raw}>{text.slice(start, end)}</pre>
  </div>;
}
