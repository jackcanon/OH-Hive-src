#!/usr/bin/env python3
"""Unit tests for cloud_card.py"""
import unittest
import tempfile
from pathlib import Path
from unittest.mock import patch

import cloud_card


class TestComputeVerdict(unittest.TestCase):
    """Test the verdict logic for all four combinations of met/not-met x should-fail/not."""
    
    def test_expectations_met_normal_case(self):
        """When expectations are met and should_fail is False, should pass."""
        passed, note = cloud_card.compute_verdict(expectations_met=True, should_fail=False)
        self.assertTrue(passed)
        self.assertEqual(note, "card produced what it claimed")
    
    def test_expectations_not_met_normal_case(self):
        """When expectations are not met and should_fail is False, should fail."""
        passed, note = cloud_card.compute_verdict(expectations_met=False, should_fail=False)
        self.assertFalse(passed)
        self.assertEqual(note, "card claimed more than it produced")
    
    def test_expectations_met_should_fail(self):
        """When expectations are met but should_fail is True, should fail (card succeeded when it shouldn't)."""
        passed, note = cloud_card.compute_verdict(expectations_met=True, should_fail=True)
        self.assertFalse(passed)
        self.assertEqual(note, "card SUCCEEDED but was expected to fall short")
    
    def test_expectations_not_met_should_fail(self):
        """When expectations are not met and should_fail is True, should pass (negative case working as expected)."""
        passed, note = cloud_card.compute_verdict(expectations_met=False, should_fail=True)
        self.assertTrue(passed)
        self.assertEqual(note, "card fell short, as the negative case expects")


class TestNodeKeyParsing(unittest.TestCase):
    """Test node key parsing from various file formats."""
    
    def test_explicit_key_takes_precedence(self):
        """If an explicit key is passed, it should be returned immediately."""
        result = cloud_card.node_key(explicit="explicit-key-123")
        self.assertEqual(result, "explicit-key-123")
    
    def test_environment_variable(self):
        """If HIVE_NODE_KEY is in environment, it should be used."""
        with patch.dict('os.environ', {'HIVE_NODE_KEY': 'env-key-456'}):
            result = cloud_card.node_key(explicit=None)
            self.assertEqual(result, "env-key-456")
    
    def test_export_prefix(self):
        """Should parse 'export HIVE_NODE_KEY=...' format."""
        with tempfile.TemporaryDirectory() as tmpdir:
            node_env = Path(tmpdir) / "node.env"
            node_env.write_text("export HIVE_NODE_KEY=test-key-export\n")
            
            with patch.object(cloud_card, 'NODE_ENV_CANDIDATES', [node_env]):
                with patch.dict('os.environ', {}, clear=True):
                    result = cloud_card.node_key(explicit=None)
                    self.assertEqual(result, "test-key-export")
    
    def test_single_quoted_value(self):
        """Should strip single quotes from the key value."""
        with tempfile.TemporaryDirectory() as tmpdir:
            node_env = Path(tmpdir) / "node.env"
            node_env.write_text("HIVE_NODE_KEY='single-quoted-key'\n")
            
            with patch.object(cloud_card, 'NODE_ENV_CANDIDATES', [node_env]):
                with patch.dict('os.environ', {}, clear=True):
                    result = cloud_card.node_key(explicit=None)
                    self.assertEqual(result, "single-quoted-key")
    
    def test_double_quoted_value(self):
        """Should strip double quotes from the key value."""
        with tempfile.TemporaryDirectory() as tmpdir:
            node_env = Path(tmpdir) / "node.env"
            node_env.write_text('HIVE_NODE_KEY="double-quoted-key"\n')
            
            with patch.object(cloud_card, 'NODE_ENV_CANDIDATES', [node_env]):
                with patch.dict('os.environ', {}, clear=True):
                    result = cloud_card.node_key(explicit=None)
                    self.assertEqual(result, "double-quoted-key")
    
    def test_no_quotes(self):
        """Should handle unquoted key values."""
        with tempfile.TemporaryDirectory() as tmpdir:
            node_env = Path(tmpdir) / "node.env"
            node_env.write_text("HIVE_NODE_KEY=unquoted-key\n")
            
            with patch.object(cloud_card, 'NODE_ENV_CANDIDATES', [node_env]):
                with patch.dict('os.environ', {}, clear=True):
                    result = cloud_card.node_key(explicit=None)
                    self.assertEqual(result, "unquoted-key")
    
    def test_file_with_no_hive_node_key_line(self):
        """When node.env exists but has no HIVE_NODE_KEY line, should return None."""
        with tempfile.TemporaryDirectory() as tmpdir:
            node_env = Path(tmpdir) / "node.env"
            node_env.write_text("# Some config file\nOTHER_VAR=value\n")
            
            with patch.object(cloud_card, 'NODE_ENV_CANDIDATES', [node_env]):
                with patch.dict('os.environ', {}, clear=True):
                    result = cloud_card.node_key(explicit=None)
                    self.assertIsNone(result)
    
    def test_nonexistent_file(self):
        """When no node.env file exists, should return None."""
        with tempfile.TemporaryDirectory() as tmpdir:
            nonexistent = Path(tmpdir) / "nonexistent" / "node.env"
            
            with patch.object(cloud_card, 'NODE_ENV_CANDIDATES', [nonexistent]):
                with patch.dict('os.environ', {}, clear=True):
                    result = cloud_card.node_key(explicit=None)
                    self.assertIsNone(result)
    
    def test_export_with_quotes(self):
        """Should handle 'export HIVE_NODE_KEY="..."' format."""
        with tempfile.TemporaryDirectory() as tmpdir:
            node_env = Path(tmpdir) / "node.env"
            node_env.write_text('export HIVE_NODE_KEY="exported-quoted-key"\n')
            
            with patch.object(cloud_card, 'NODE_ENV_CANDIDATES', [node_env]):
                with patch.dict('os.environ', {}, clear=True):
                    result = cloud_card.node_key(explicit=None)
                    self.assertEqual(result, "exported-quoted-key")
    
    def test_multiple_lines_with_comments(self):
        """Should find HIVE_NODE_KEY in a file with multiple lines and comments."""
        with tempfile.TemporaryDirectory() as tmpdir:
            node_env = Path(tmpdir) / "node.env"
            node_env.write_text("""# Node configuration
# Generated by hive
OTHER_SETTING=foo
HIVE_NODE_KEY=multi-line-key
ANOTHER_SETTING=bar
""")
            
            with patch.object(cloud_card, 'NODE_ENV_CANDIDATES', [node_env]):
                with patch.dict('os.environ', {}, clear=True):
                    result = cloud_card.node_key(explicit=None)
                    self.assertEqual(result, "multi-line-key")
    
    def test_first_candidate_takes_precedence(self):
        """When multiple candidate files exist, the first one should be used."""
        with tempfile.TemporaryDirectory() as tmpdir:
            first_env = Path(tmpdir) / "first.env"
            first_env.write_text("HIVE_NODE_KEY=first-key\n")
            
            second_env = Path(tmpdir) / "second.env"
            second_env.write_text("HIVE_NODE_KEY=second-key\n")
            
            with patch.object(cloud_card, 'NODE_ENV_CANDIDATES', [first_env, second_env]):
                with patch.dict('os.environ', {}, clear=True):
                    result = cloud_card.node_key(explicit=None)
                    self.assertEqual(result, "first-key")


if __name__ == '__main__':
    unittest.main()
