import { CodeBlock } from '@rspress/core/theme';
import { Fragment } from 'react';
import { tokenizeCode, type CodeExampleLanguage } from './CodeExample.tokens';

export function CodeExample({
  code,
  containerElementClassName,
  height,
  lang,
  lineNumbers = false,
  title,
  wrapCode = false,
}: {
  code: string;
  containerElementClassName?: string;
  height?: number;
  lang: CodeExampleLanguage;
  lineNumbers?: boolean;
  title?: string;
  wrapCode?: boolean;
}) {
  const lines = code.split('\n');

  return (
    <CodeBlock
      containerElementClassName={containerElementClassName}
      height={height}
      lang={lang}
      lineNumbers={lineNumbers}
      title={title}
      wrapCode={wrapCode}
    >
      <pre className="shiki css-variables flow-code-example" data-lang={lang}>
        <code>
          {lines.map((line, lineIndex) => (
            <Fragment key={`${lineIndex}-${line}`}>
              <span className="line">
                {tokenizeCode(line, lang).map((token, tokenIndex) =>
                  token.kind === 'plain' ? (
                    token.value
                  ) : (
                    <span
                      className={`flow-code-token flow-code-token--${token.kind}`}
                      key={`${tokenIndex}-${token.value}`}
                    >
                      {token.value}
                    </span>
                  ),
                )}
              </span>
              {lineIndex < lines.length - 1 ? '\n' : null}
            </Fragment>
          ))}
        </code>
      </pre>
    </CodeBlock>
  );
}
