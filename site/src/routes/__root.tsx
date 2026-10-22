import { createRootRoute, HeadContent, Outlet, Scripts } from '@tanstack/react-router'
const themeScript = `try{const t=localStorage.getItem('milleleaf-theme');if(t==='light'||t==='dark')document.documentElement.dataset.theme=t}catch{}`
export const Route = createRootRoute({
  head: () => ({
    meta: [{ charSet: 'utf-8' }, { name: 'viewport', content: 'width=device-width, initial-scale=1' }, { name: 'milleleaf-site', content: 'website' }, { name: 'description', content: 'Make content once and use it across apps, with reviewable changes, signed layers and loss reports. Explore the tool, WorldViews, public data and the evidence method.' }],
    links: [{ rel: 'stylesheet', href: '/styles.css' }, { rel: 'icon', type: 'image/svg+xml', href: '/brand/milleleaf-mark.svg' }],
  }),
  notFoundComponent: () => <main style={{ padding: '4rem' }}><h1>Page not found</h1><a href="/">Back to Milleleaf</a></main>,
  component: () => <html lang="en" suppressHydrationWarning><head><script dangerouslySetInnerHTML={{ __html: themeScript }} /><HeadContent /></head><body><Outlet /><Scripts /></body></html>,
})
