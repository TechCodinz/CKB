import React, { useState, useContext } from 'react';
import { useNavigate, Link } from 'react-router-dom';
import {
    Box, Card, CardContent, Typography, TextField,
    Button, CircularProgress, Alert, Divider
} from '@mui/material';
import { AuthContext } from '../App';
import { ckbApi } from '../services/api';

export default function Login() {
    const navigate = useNavigate();
    const { login } = useContext(AuthContext);
    const [email, setEmail] = useState('');
    const [password, setPassword] = useState('');
    const [loading, setLoading] = useState(false);
    const [error, setError] = useState('');

    const handleSubmit = async (e: React.FormEvent) => {
        e.preventDefault();
        setLoading(true);
        setError('');
        try {
            const resp = await ckbApi.login(email, password);
            const { token } = resp.data;
            localStorage.setItem('ckb_token', token);
            login();
            navigate('/');
        } catch (err: any) {
            setError(err?.response?.data?.message || 'Invalid email or password');
        } finally {
            setLoading(false);
        }
    };

    return (
        <Box sx={{
            minHeight: '100vh',
            background: 'linear-gradient(135deg, #0a0a1a 0%, #1a0a2e 50%, #0a1a2e 100%)',
            display: 'flex', alignItems: 'center', justifyContent: 'center',
        }}>
            <Card sx={{
                width: '100%', maxWidth: 420,
                background: 'rgba(255,255,255,0.05)',
                backdropFilter: 'blur(20px)',
                border: '1px solid rgba(255,255,255,0.1)',
                borderRadius: 3,
            }}>
                <CardContent sx={{ p: 4 }}>
                    <Box sx={{ textAlign: 'center', mb: 4 }}>
                        <Typography variant="h4" sx={{
                            fontWeight: 800,
                            background: 'linear-gradient(90deg, #90caf9 0%, #ce93d8 100%)',
                            WebkitBackgroundClip: 'text', WebkitTextFillColor: 'transparent'
                        }}>
                            CKB
                        </Typography>
                        <Typography color="textSecondary" variant="body2">
                            Code Knowledge Base
                        </Typography>
                    </Box>

                    <Typography variant="h6" fontWeight={600} mb={3}>Sign in to your account</Typography>

                    {error && <Alert severity="error" sx={{ mb: 2 }}>{error}</Alert>}

                    <Box component="form" onSubmit={handleSubmit} sx={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
                        <TextField
                            id="email"
                            label="Email"
                            type="email"
                            value={email}
                            onChange={e => setEmail(e.target.value)}
                            required
                            fullWidth
                            autoFocus
                            InputLabelProps={{ shrink: true }}
                        />
                        <TextField
                            id="password"
                            label="Password"
                            type="password"
                            value={password}
                            onChange={e => setPassword(e.target.value)}
                            required
                            fullWidth
                            InputLabelProps={{ shrink: true }}
                        />
                        <Button
                            type="submit"
                            variant="contained"
                            fullWidth
                            disabled={loading}
                            sx={{
                                mt: 1, py: 1.5, fontWeight: 700,
                                background: 'linear-gradient(90deg, #90caf9 0%, #ce93d8 100%)',
                                color: '#000',
                            }}
                        >
                            {loading ? <CircularProgress size={22} /> : 'Sign In'}
                        </Button>

                        <Divider sx={{ my: 1 }} />

                        <Typography variant="body2" textAlign="center" color="textSecondary">
                            No account?{' '}
                            <Link to="/signup" style={{ color: '#90caf9', textDecoration: 'none', fontWeight: 600 }}>
                                Start free trial
                            </Link>
                        </Typography>
                    </Box>
                </CardContent>
            </Card>
        </Box>
    );
}
