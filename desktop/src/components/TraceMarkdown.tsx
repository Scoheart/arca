import { memo, useMemo, useState } from 'react';
import Markdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { openUrl } from '@tauri-apps/plugin-opener';
import { useTranslation } from 'react-i18next';
import { textSegments } from '../lib/textSegments';
import { formatError } from '../lib/format';
import { useToast } from './Toast';

/** Logs are untrusted: no HTML, image/network fetches, scripts or local paths.
 * Large messages use the same bounded text segments as the raw inspector. */
export const TraceMarkdown = memo(function TraceMarkdown({ text }: { text: string }) {
  const { t } = useTranslation();
  const toast = useToast();
  const offsets = useMemo(() => textSegments(text), [text]);
  const [selected, setSelected] = useState(0);
  const page = Math.min(selected, offsets.length - 1);
  return <div className="arca-trace-prose">
    {offsets.length > 1 && <div className="arca-trace-segments" aria-label={t('textSegments.label')}>
      <span>{t('textSegments.position', { current: page + 1, total: offsets.length })}</span>
      <button type="button" disabled={page === 0} onClick={() => setSelected(page - 1)}>{t('textSegments.previous')}</button>
      <button type="button" disabled={page + 1 === offsets.length} onClick={() => setSelected(page + 1)}>{t('textSegments.next')}</button>
    </div>}
    <Markdown remarkPlugins={[remarkGfm]} skipHtml components={{
      a: ({ href, children }) => href && /^(https?:|mailto:)/i.test(href)
        ? <button type="button" className="arca-trace-link" title={href} onClick={() => void openUrl(href)
          .catch(error => toast.show({ kind: 'error', msg: formatError(error) }))}>{children}</button>
        : <span>{children}</span>,
      img: ({ alt }) => <span className="arca-trace-attachment">{t('trace.imageAttachment')}{alt ? ` · ${alt}` : ''}</span>,
      table: ({ children }) => <div className="arca-trace-table-scroll"><table>{children}</table></div>,
      input: ({ checked }) => <span aria-hidden="true">{checked ? '☑' : '☐'}</span>,
    }}>{text.slice(offsets[page], offsets[page + 1] ?? text.length)}</Markdown>
  </div>;
});
