package dev.nglmercer.tiktools.data.database;

import androidx.room.*;
import java.util.List;
import kotlinx.coroutines.flow.Flow;

@Dao
public interface AutomationDao {
  @Query("SELECT * FROM actions ORDER BY id")
  Flow<List<ActionRow>> observeActions();

  @Insert(onConflict = OnConflictStrategy.REPLACE)
  long save(ActionRow action);

  @Query("DELETE FROM actions WHERE id=:id")
  void delete(long id);
}
