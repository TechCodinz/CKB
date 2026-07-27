import React, { useState, useMemo, useEffect } from 'react';
import { BrowserRouter as Router, Routes, Route, Navigate } from 'react-router-dom';
import { ThemeProvider, createTheme, CssBaseline } from '@mui/material';
import Dashboard from './pages/Dashboard';
import ProjectView from './pages/ProjectView';
import GraphView from './pages/GraphView';
import Login from './pages/Login';
import Signup from './pages/Signup';

interface AuthCtx {
  isAuthenticated: boolean;
  login: () => void;
  logout: () => void;
}

interface ProjectCtx {
  currentProject: any;
  setCurrentProject: (p: any) => void;
}

export const AuthContext = React.createContext<AuthCtx>({
  isAuthenticated: false,
  login: () => { },
  logout: () => { },
});

export const ProjectContext = React.createContext<ProjectCtx>({
  currentProject: null,
  setCurrentProject: () => { },
});

function PrivateRoute({ children }: { children: React.ReactNode }) {
  const token = localStorage.getItem('ckb_token');
  return token ? <>{children}</> : <Navigate to="/login" replace />;
}

function App() {
  const [isAuthenticated, setIsAuthenticated] = useState(!!localStorage.getItem('ckb_token'));
  const [currentProject, setCurrentProject] = useState(null);

  useEffect(() => {
    const handleStorageChange = () => {
      setIsAuthenticated(!!localStorage.getItem('ckb_token'));
    };
    window.addEventListener('storage', handleStorageChange);
    return () => window.removeEventListener('storage', handleStorageChange);
  }, []);

  const theme = useMemo(
    () =>
      createTheme({
        palette: {
          mode: 'dark',
          primary: { main: '#90caf9' },
          secondary: { main: '#ce93d8' },
          background: { default: '#0d1117', paper: '#161b22' },
        },
        typography: {
          fontFamily: '"Inter", "Roboto", sans-serif',
        },
        components: {
          MuiCard: {
            styleOverrides: {
              root: { backgroundImage: 'none' }
            }
          }
        }
      }),
    []
  );

  const login = () => setIsAuthenticated(true);
  const logout = () => {
    localStorage.removeItem('ckb_token');
    setIsAuthenticated(false);
  };

  return (
    <ThemeProvider theme={theme}>
      <CssBaseline />
      <AuthContext.Provider value={{ isAuthenticated, login, logout }}>
        <ProjectContext.Provider value={{ currentProject, setCurrentProject }}>
          <Router>
            <Routes>
              <Route path="/login" element={<Login />} />
              <Route path="/signup" element={<Signup />} />
              <Route path="/" element={<PrivateRoute><Dashboard /></PrivateRoute>} />
              <Route path="/project/:id" element={<PrivateRoute><ProjectView /></PrivateRoute>} />
              <Route path="/project/:id/graph" element={<PrivateRoute><GraphView /></PrivateRoute>} />
              <Route path="*" element={<Navigate to="/" replace />} />
            </Routes>
          </Router>
        </ProjectContext.Provider>
      </AuthContext.Provider>
    </ThemeProvider>
  );
}

export default App;
