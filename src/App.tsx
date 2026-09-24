import { BrowserRouter, Routes, Route, Navigate } from "react-router-dom";
import { Toaster } from "sonner";
import { AppProvider } from "./context/AppContext";
import { ThemeProvider, useThemeContext } from "./context/ThemeContext";
import { HelpDialog } from "./components/HelpDialog";
import { Layout } from "./components/Layout";
import { MySkills } from "./views/MySkills";
import { InstallSkills } from "./views/InstallSkills";
import { Inventory } from "./views/Inventory";
import { Settings } from "./views/Settings";
import { ProjectDetail } from "./views/ProjectDetail";

function ThemedToaster() {
  const { resolvedTheme } = useThemeContext();
  return (
    <Toaster
      theme={resolvedTheme}
      position="bottom-right"
      toastOptions={{
        style: {
          background: "var(--color-surface)",
          border: "1px solid var(--color-border)",
          color: "var(--color-text-primary)",
        },
      }}
    />
  );
}

function App() {
  return (
    <ThemeProvider>
      <AppProvider>
        <BrowserRouter>
          <Routes>
            <Route element={<Layout />}>
              <Route path="/" element={<Navigate to="/my-skills" replace />} />
              <Route path="/inventory" element={<Inventory />} />
              <Route path="/my-skills" element={<MySkills />} />
              <Route path="/install" element={<InstallSkills />} />
              <Route path="/project/:id" element={<ProjectDetail />} />
              <Route path="/settings" element={<Settings />} />
            </Route>
          </Routes>
          <HelpDialog />
        </BrowserRouter>
        <ThemedToaster />
      </AppProvider>
    </ThemeProvider>
  );
}

export default App;
