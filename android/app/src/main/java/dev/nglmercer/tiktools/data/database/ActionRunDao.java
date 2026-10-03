package dev.nglmercer.tiktools.data.database;

import androidx.room.*;
import java.util.List;
import kotlinx.coroutines.flow.Flow;

@Dao
public interface ActionRunDao {
  @Query("SELECT * FROM runs ORDER BY id DESC LIMIT 100")
  Flow<List<RunRow>> observeRuns();

  @Insert
  long insert(RunRow row);

  @Query("DELETE FROM runs WHERE id NOT IN (SELECT id FROM runs ORDER BY id DESC LIMIT 100)")
  void trim();
}
