import { useState } from 'react'
import { Tab, TabGroup, TabList, TabPanel, TabPanels } from '@headlessui/react'

interface CompareResult {
  pandoc: any;
  qmd: any;
  pandocError?: string;
  qmdError?: string;
}

function App() {
  const [markdown, setMarkdown] = useState('')
  const [result, setResult] = useState<CompareResult | null>(null)
  const [loading, setLoading] = useState(false)
  const [removeLocations, setRemoveLocations] = useState(true)

  const removeLocationInfo = (obj: any): any => {
    if (obj === null || obj === undefined) {
      return obj;
    }
    if (Array.isArray(obj)) {
      return obj.map(removeLocationInfo);
    }
    if (typeof obj === 'object') {
      const newObj: any = {};
      for (const key in obj) {
        if (key !== 'l') {
          newObj[key] = removeLocationInfo(obj[key]);
        }
      }
      return newObj;
    }
    return obj;
  }

  const sortKeys = (obj: any): any => {
    if (obj === null || obj === undefined) {
      return obj;
    }
    if (Array.isArray(obj)) {
      return obj.map(sortKeys);
    }
    if (typeof obj === 'object') {
      const sorted: any = {};
      const keys = Object.keys(obj).sort();
      for (const key of keys) {
        sorted[key] = sortKeys(obj[key]);
      }
      return sorted;
    }
    return obj;
  }

  const handleCompare = async () => {
    setLoading(true)
    try {
      const response = await fetch('http://localhost:3001/compare', {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
        },
        body: JSON.stringify({ markdown }),
      })
      const data = await response.json()
      setResult(data)
    } catch (error) {
      console.error('Error comparing:', error)
    } finally {
      setLoading(false)
    }
  }

  return (
    <div className="max-w-7xl mx-auto p-8">
      <h1 className="text-3xl font-bold text-center mb-8">Pandoc vs Quarto Markdown Diff Tool</h1>

      <div className="mb-8">
        <h2 className="text-xl font-semibold mb-4">Markdown Input</h2>
        <textarea
          value={markdown}
          onChange={(e) => setMarkdown(e.target.value)}
          onKeyDown={(e) => {
            if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') {
              e.preventDefault();
              if (markdown && !loading) {
                handleCompare();
              }
            }
          }}
          placeholder="Enter markdown here..."
          rows={10}
          className="w-full p-3 font-mono text-sm border border-gray-300 dark:border-gray-600 rounded bg-white dark:bg-gray-900 text-black dark:text-white mb-4"
        />
        <div className="flex items-center gap-4 mb-4">
          <label className="flex items-center gap-2 cursor-pointer">
            <input
              type="checkbox"
              checked={removeLocations}
              onChange={(e) => setRemoveLocations(e.target.checked)}
              className="w-4 h-4 cursor-pointer"
            />
            <span>Remove location info (l entries)</span>
          </label>
        </div>
        <button
          onClick={handleCompare}
          disabled={loading || !markdown}
          className="px-8 py-3 bg-indigo-600 text-white rounded hover:bg-indigo-700 disabled:bg-gray-400 disabled:cursor-not-allowed transition-colors"
        >
          {loading ? 'Comparing...' : 'Compare'}
        </button>
      </div>

      {result && (
        <TabGroup>
          <TabList className="flex gap-2 border-b-2 border-gray-300 dark:border-gray-600">
            <Tab className="px-6 py-3 text-gray-600 dark:text-gray-400 border-b-2 border-transparent -mb-0.5 data-[selected]:text-indigo-600 data-[selected]:border-indigo-600 hover:text-indigo-500 transition-colors">
              Pandoc Output
            </Tab>
            <Tab className="px-6 py-3 text-gray-600 dark:text-gray-400 border-b-2 border-transparent -mb-0.5 data-[selected]:text-indigo-600 data-[selected]:border-indigo-600 hover:text-indigo-500 transition-colors">
              Quarto Markdown Output
            </Tab>
          </TabList>
          <TabPanels className="mt-4">
            <TabPanel>
              {result.pandocError ? (
                <div className="text-red-700 dark:text-red-400 bg-red-50 dark:bg-red-950 p-4 rounded font-mono whitespace-pre-wrap">
                  {result.pandocError}
                </div>
              ) : (
                <pre className="overflow-x-auto bg-gray-100 dark:bg-gray-900 text-black dark:text-white p-4 rounded text-xs max-h-[600px] overflow-y-auto border border-gray-300 dark:border-gray-600">
                  {JSON.stringify(sortKeys(removeLocations ? removeLocationInfo(result.pandoc) : result.pandoc), null, 2)}
                </pre>
              )}
            </TabPanel>
            <TabPanel>
              {result.qmdError ? (
                <div className="text-red-700 dark:text-red-400 bg-red-50 dark:bg-red-950 p-4 rounded font-mono whitespace-pre-wrap">
                  {result.qmdError}
                </div>
              ) : (
                <pre className="overflow-x-auto bg-gray-100 dark:bg-gray-900 text-black dark:text-white p-4 rounded text-xs max-h-[600px] overflow-y-auto border border-gray-300 dark:border-gray-600">
                  {JSON.stringify(sortKeys(removeLocations ? removeLocationInfo(result.qmd) : result.qmd), null, 2)}
                </pre>
              )}
            </TabPanel>
          </TabPanels>
        </TabGroup>
      )}
    </div>
  )
}

export default App
