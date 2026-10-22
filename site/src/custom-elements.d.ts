import type { DetailedHTMLProps, HTMLAttributes } from 'react'
declare module 'react' {
  namespace JSX {
    interface IntrinsicElements {
      'ml-theme': DetailedHTMLProps<HTMLAttributes<HTMLElement>, HTMLElement>
      'ml-demo': DetailedHTMLProps<HTMLAttributes<HTMLElement>, HTMLElement>
      'ml-layers': DetailedHTMLProps<HTMLAttributes<HTMLElement>, HTMLElement>
    }
  }
}
